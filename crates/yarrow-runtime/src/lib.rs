//! Host runtime for Yarrow values (strings, lists, hashmaps) plus the symbol
//! table that exposes these functions to JIT-compiled code.
//!
//! Values are opaque `u64` handles that point to heap-allocated headers whose
//! layout is private to this module; compiled code only ever passes handles
//! around and calls back into the runtime to inspect or mutate them.
//!
//! # Ownership, regions and frees
//!
//! Heap values are freed through [`yarrow_free_value`], which recurses into
//! nested values using a compiler-produced *kind code* (see [`Kind`]). The
//! compiler also registers the layout of every struct (via
//! [`yarrow_register_struct_descs`]) so structs can be freed field by field.
//!
//! [`yarrow_region_register`] attaches a value to a region; freeing the region
//! frees every value attached to it as a unit. A global registry of attached
//! values lets ordinary frees detach a value from its region first, and a set
//! of already-freed handles guards against double frees (e.g. a value freed by
//! a region and again by a variable drop at scope exit).

// Every function in this module is unsafe by design (raw pointers to the heap
// header structures); allow the explicit inner-unsafe lint.
#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Kind codes
// ---------------------------------------------------------------------------

/// Type codes used by `yarrow_free_value` / `yarrow_region_register` to decide
/// how to recurse into a value. These are produced by the compiler
/// (`Compiler::encode_kind`); the encoding here must stay in sync.
///
/// * `0..=15` — scalar (nothing to free).
/// * `16` — a string handle.
/// * `0x20` — a list; element kind in bits 8..; the low byte is the tag.
/// * `0x30` — a hashmap; key kind in bits 8.., value kind in bits 40.. .
/// * `0x40` — a struct; struct layout id in bits 8.. .
/// * `0x50` — a generic pointer (cannot recurse).
/// * `0x60` — a fixed-size array; element kind in bits 8.. .
/// * `0x70` — a union; union id in bits 8.. . The union block stores the
///   active member's index (a tag) and an inline payload; the member kind
///   codes are registered via `yarrow_register_union_descs`.
/// * `0x80` — an enum; enum id in bits 8.. . The stored bits are the
///   carrier value (an integer discriminant), so there is nothing to free.
pub const KIND_STRING: u64 = 16;
pub const KIND_LIST: u64 = 0x20;
pub const KIND_MAP: u64 = 0x30;
pub const KIND_STRUCT: u64 = 0x40;
pub const KIND_PTR: u64 = 0x50;
pub const KIND_ARRAY: u64 = 0x60;
pub const KIND_UNION: u64 = 0x70;
pub const KIND_ENUM: u64 = 0x80;

/// Byte offsets inside a union block, in sync with the compiler's
/// `emit_union_wrap`.
pub const UNION_TAG_OFFSET: u64 = 0;
pub const UNION_PAYLOAD_OFFSET: u64 = 8;

#[inline]
fn tag(kind: u64) -> u64 {
    kind & 0xff
}

// ---------------------------------------------------------------------------
// Headers
// ---------------------------------------------------------------------------

#[repr(C)]
struct Str {
    ptr: *mut u8,
    len: usize,
}

#[repr(C)]
struct List {
    len: usize,
    cap: usize,
    elem_size: usize,
    data: *mut u8,
}

#[repr(C)]
struct Map {
    len: usize,
    cap: usize,
    keys_string: u8,
    _pad: [u8; 7],
    keys: *mut u64,
    vals: *mut u64,
    used: *mut u8,
}

/// A heap region: an owning set of values freed as a unit.
#[repr(C)]
struct Region {
    cap: usize,
    len: usize,
    values: *mut RegionEntry,
}

/// One registered value: the handle plus the kind code needed to free it.
#[repr(C)]
#[derive(Clone, Copy)]
struct RegionEntry {
    value: u64,
    kind: u64,
}

/// A single struct field's layout as registered by the compiler.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FieldDesc {
    pub offset: u32,
    pub _pad: u32,
    pub kind: u64,
}

// ---------------------------------------------------------------------------
// Global bookkeeping
// ---------------------------------------------------------------------------

/// Value handle -> the region it is currently attached to. Only *top-level*
/// values registered by `@put_region` appear here; nested values are reached
/// through their parent and are freed with it.
static REGISTRY: std::sync::LazyLock<Mutex<HashMap<u64, u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Handles already freed, so a second free of the same handle is a no-op.
/// Without a GC this is how the runtime survives cases like a region free
/// followed by a variable drop of the same value.
static FREED: std::sync::LazyLock<Mutex<std::collections::HashSet<u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// Struct layout id -> registered field descriptors.
static STRUCT_DESCS: std::sync::LazyLock<Mutex<HashMap<u32, Vec<FieldDesc>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Union id -> registered member kind codes (one per member, indexed by the
/// union's active-member tag).
static UNION_DESCS: std::sync::LazyLock<Mutex<HashMap<u32, Vec<u64>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Enum id -> registered carrier kind code. A string carrier stores a heap
/// string handle per value; an integer carrier stores inline scalar bits.
static ENUM_DESCS: std::sync::LazyLock<Mutex<HashMap<u32, u64>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Heap
// ---------------------------------------------------------------------------

pub extern "C" fn yarrow_alloc(size: u64) -> u64 {
    unsafe { libc::malloc(size as usize) as u64 }
}

/// Host `free` for `@free` / drop paths.
pub extern "C" fn yarrow_free(ptr: u64) {
    unsafe {
        libc::free(ptr as *mut libc::c_void);
    }
}

/// Detach `handle` from whatever region it is attached to (no-op if none).
/// Called before freeing a value so a later `@free_region` won't free it again.
fn detach(handle: u64) {
    let region = match REGISTRY.lock().unwrap().remove(&handle) {
        Some(r) => r,
        None => return,
    };
    let region_ptr = region as *mut Region;
    if region_ptr.is_null() {
        return;
    }
    unsafe {
        let reg = &mut *region_ptr;
        let n = reg.len;
        for i in 0..n {
            if (*reg.values.add(i)).value == handle {
                // Move the last entry into this slot and shrink.
                let last = *reg.values.add(n - 1);
                *reg.values.add(i) = last;
                reg.len -= 1;
                break;
            }
        }
    }
}

/// Mark `handle` as freed, guarding against double frees.
fn mark_freed(handle: u64) {
    FREED.lock().unwrap().insert(handle);
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

/// Build a string from a byte buffer (copied; must not be freed by the
/// caller). Returns a handle to the header.
pub extern "C" fn yarrow_str_new(ptr: u64, len: u64) -> u64 {
    let buf = unsafe { libc::malloc(len as usize + 1) } as *mut u8;
    unsafe {
        std::ptr::copy_nonoverlapping(ptr as *const u8, buf, len as usize);
        *buf.add(len as usize) = 0;
    }
    let hdr = unsafe { libc::malloc(std::mem::size_of::<Str>()) } as *mut Str;
    unsafe {
        *hdr = Str {
            ptr: buf,
            len: len as usize,
        };
    }
    hdr as u64
}

pub extern "C" fn yarrow_str_len(s: u64) -> u64 {
    unsafe { (*(s as *const Str)).len as u64 }
}

/// Copy a string handle's bytes into a Rust `Vec<u8>` (used by `run_main` to
/// surface a `string` result to the driver). Returns `None` for a null handle.
pub fn string_bytes(s: u64) -> Option<Vec<u8>> {
    if s == 0 {
        return None;
    }
    unsafe {
        let str = &*(s as *const Str);
        Some(std::slice::from_raw_parts(str.ptr, str.len).to_vec())
    }
}

pub extern "C" fn yarrow_str_join(a: u64, b: u64) -> u64 {
    unsafe {
        let sa = &*(a as *const Str);
        let sb = &*(b as *const Str);
        let buf = libc::malloc(sa.len + sb.len + 1) as *mut u8;
        std::ptr::copy_nonoverlapping(sa.ptr, buf, sa.len);
        std::ptr::copy_nonoverlapping(sb.ptr, buf.add(sa.len), sb.len);
        *buf.add(sa.len + sb.len) = 0;
        let hdr = libc::malloc(std::mem::size_of::<Str>()) as *mut Str;
        *hdr = Str {
            ptr: buf,
            len: sa.len + sb.len,
        };
        hdr as u64
    }
}

/// Lexicographic comparison: -1, 0 or 1.
pub extern "C" fn yarrow_str_cmp(a: u64, b: u64) -> i64 {
    unsafe {
        let sa = &*(a as *const Str);
        let sb = &*(b as *const Str);
        let n = sa.len.min(sb.len);
        let ab = std::slice::from_raw_parts(sa.ptr, n);
        let bb = std::slice::from_raw_parts(sb.ptr, n);
        match ab.cmp(bb) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Equal => sa.len.cmp(&sb.len) as i64,
        }
    }
}

fn free_str(s: u64) {
    if s == 0 || FREED.lock().unwrap().contains(&s) {
        return;
    }
    detach(s);
    unsafe {
        let str = &mut *(s as *mut Str);
        libc::free(str.ptr as *mut libc::c_void);
        libc::free(s as *mut libc::c_void);
    }
    mark_freed(s);
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

pub extern "C" fn yarrow_list_new(elem_size: u64) -> u64 {
    let hdr = unsafe { libc::malloc(std::mem::size_of::<List>()) } as *mut List;
    unsafe {
        *hdr = List {
            len: 0,
            cap: 0,
            elem_size: elem_size as usize,
            data: std::ptr::null_mut(),
        };
    }
    hdr as u64
}

pub extern "C" fn yarrow_list_len(l: u64) -> u64 {
    unsafe { (*(l as *const List)).len as u64 }
}

pub extern "C" fn yarrow_list_get(l: u64, index: u64) -> u64 {
    unsafe {
        let list = &*(l as *const List);
        if index >= list.len as u64 {
            return 0;
        }
        let src = list.data.add(index as usize * list.elem_size);
        let mut value: u64 = 0;
        std::ptr::copy_nonoverlapping(src, &mut value as *mut u64 as *mut u8, list.elem_size);
        value
    }
}

pub extern "C" fn yarrow_list_push(l: u64, value: u64) {
    unsafe {
        let list = &mut *(l as *mut List);
        if list.len == list.cap {
            let new_cap = if list.cap == 0 { 4 } else { list.cap * 2 };
            list.data =
                libc::realloc(list.data as *mut libc::c_void, new_cap * list.elem_size) as *mut u8;
            list.cap = new_cap;
        }
        let dst = list.data.add(list.len * list.elem_size);
        let src = (&value as *const u64) as *const u8;
        std::ptr::copy_nonoverlapping(src, dst, list.elem_size);
        list.len += 1;
    }
}

/// Overwrite the element at `index` with `value`. No-op when `index` is out of
/// range; callers bounds-check before calling.
pub extern "C" fn yarrow_list_set(l: u64, index: u64, value: u64) {
    unsafe {
        let list = &mut *(l as *mut List);
        if index >= list.len as u64 {
            return;
        }
        let dst = list.data.add(index as usize * list.elem_size);
        let src = (&value as *const u64) as *const u8;
        std::ptr::copy_nonoverlapping(src, dst, list.elem_size);
    }
}

/// Free a list and every element, using `elem_kind` to decide how to recurse.
fn free_list(l: u64, elem_kind: u64) {
    if l == 0 || FREED.lock().unwrap().contains(&l) {
        return;
    }
    detach(l);
    unsafe {
        let list = &*(l as *const List);
        if tag(elem_kind) > 15 && elem_kind != KIND_PTR && !list.data.is_null() {
            for i in 0..list.len {
                let elem = read_elem(list, i);
                free_value(elem, elem_kind);
            }
        }
        libc::free(list.data as *mut libc::c_void);
        libc::free(l as *mut libc::c_void);
    }
    mark_freed(l);
}

unsafe fn read_elem(list: &List, i: usize) -> u64 {
    unsafe {
        let p = list.data.add(i * list.elem_size);
        match list.elem_size {
            1 => *p as u64,
            2 => *(p as *const u16) as u64,
            4 => *(p as *const u32) as u64,
            _ => *(p as *const u64),
        }
    }
}

// ---------------------------------------------------------------------------
// Hashmaps (linear probing; keys are either integers or string handles)
// ---------------------------------------------------------------------------

unsafe fn hash_key(map: &Map, key: u64) -> u64 {
    if map.keys_string != 0 {
        let s = unsafe { &*(key as *const Str) };
        let bytes = unsafe { std::slice::from_raw_parts(s.ptr, s.len) };
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    } else {
        key.wrapping_mul(0x9E3779B97F4A7C15)
    }
}

unsafe fn key_eq(map: &Map, a: u64, b: u64) -> bool {
    if map.keys_string != 0 {
        let sa = unsafe { &*(a as *const Str) };
        let sb = unsafe { &*(b as *const Str) };
        sa.len == sb.len
            && unsafe { std::slice::from_raw_parts(sa.ptr, sa.len) }
                == unsafe { std::slice::from_raw_parts(sb.ptr, sb.len) }
    } else {
        a == b
    }
}

unsafe fn map_grow(map: &mut Map) {
    let new_cap = if map.cap == 0 { 8 } else { map.cap * 2 };
    let new_keys = libc::calloc(new_cap, std::mem::size_of::<u64>()) as *mut u64;
    let new_vals = libc::calloc(new_cap, std::mem::size_of::<u64>()) as *mut u64;
    let new_used = libc::calloc(new_cap, 1) as *mut u8;
    for i in 0..map.cap {
        if *map.used.add(i) == 0 {
            continue;
        }
        let key = *map.keys.add(i);
        let val = *map.vals.add(i);
        let mut idx = (hash_key(map, key) % new_cap as u64) as usize;
        while *new_used.add(idx) != 0 {
            idx = (idx + 1) % new_cap;
        }
        *new_keys.add(idx) = key;
        *new_vals.add(idx) = val;
        *new_used.add(idx) = 1;
    }
    libc::free(map.keys as *mut libc::c_void);
    libc::free(map.vals as *mut libc::c_void);
    libc::free(map.used as *mut libc::c_void);
    map.keys = new_keys;
    map.vals = new_vals;
    map.used = new_used;
    map.cap = new_cap;
}

pub extern "C" fn yarrow_map_new(keys_string: u64) -> u64 {
    let hdr = unsafe { libc::malloc(std::mem::size_of::<Map>()) } as *mut Map;
    unsafe {
        *hdr = Map {
            len: 0,
            cap: 0,
            keys_string: keys_string as u8,
            _pad: [0; 7],
            keys: std::ptr::null_mut(),
            vals: std::ptr::null_mut(),
            used: std::ptr::null_mut(),
        };
    }
    hdr as u64
}

pub extern "C" fn yarrow_map_insert(m: u64, key: u64, value: u64) {
    unsafe {
        let map = &mut *(m as *mut Map);
        if map.cap == 0 || map.len as f64 > map.cap as f64 * 0.7 {
            map_grow(map);
        }
        let mut idx = (hash_key(map, key) % map.cap as u64) as usize;
        loop {
            if *map.used.add(idx) == 0 {
                *map.keys.add(idx) = key;
                *map.vals.add(idx) = value;
                *map.used.add(idx) = 1;
                map.len += 1;
                return;
            }
            if key_eq(map, *map.keys.add(idx), key) {
                *map.vals.add(idx) = value;
                return;
            }
            idx = (idx + 1) % map.cap;
        }
    }
}

/// Look up `key`; sets `*found` to 1/0 and returns the stored value (0 when
/// absent).
///
/// # Safety
///
/// `m` must be a valid map handle and `found` a valid writable pointer.
pub unsafe extern "C" fn yarrow_map_get(m: u64, key: u64, found: *mut u8) -> u64 {
    unsafe {
        let map = &*(m as *const Map);
        if map.cap == 0 {
            *found = 0;
            return 0;
        }
        let mut idx = (hash_key(map, key) % map.cap as u64) as usize;
        loop {
            if *map.used.add(idx) == 0 {
                *found = 0;
                return 0;
            }
            if key_eq(map, *map.keys.add(idx), key) {
                *found = 1;
                return *map.vals.add(idx);
            }
            idx = (idx + 1) % map.cap;
        }
    }
}

pub extern "C" fn yarrow_map_len(m: u64) -> u64 {
    unsafe { (*(m as *const Map)).len as u64 }
}

/// Free a map and every key/value, using `key_kind`/`val_kind` to recurse.
fn free_map(m: u64, key_kind: u64, val_kind: u64) {
    if m == 0 || FREED.lock().unwrap().contains(&m) {
        return;
    }
    detach(m);
    unsafe {
        let map = &*(m as *const Map);
        for i in 0..map.cap {
            if *map.used.add(i) == 0 {
                continue;
            }
            let key = *map.keys.add(i);
            let val = *map.vals.add(i);
            if tag(key_kind) > 15 && key_kind != KIND_PTR {
                free_value(key, key_kind);
            }
            if tag(val_kind) > 15 && val_kind != KIND_PTR {
                free_value(val, val_kind);
            }
        }
        libc::free(map.keys as *mut libc::c_void);
        libc::free(map.vals as *mut libc::c_void);
        libc::free(map.used as *mut libc::c_void);
        libc::free(m as *mut libc::c_void);
    }
    mark_freed(m);
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Free a struct value's heap fields, then the struct storage itself. Struct
/// instances are heap-allocated by the compiler (`yarrow_alloc`), so the
/// pointer is released here rather than recovered from a frame slot.
fn free_struct(s: u64, id: u32) {
    if s == 0 || FREED.lock().unwrap().contains(&s) {
        return;
    }
    detach(s);
    let descs = match STRUCT_DESCS.lock().unwrap().get(&id) {
        Some(d) => d.clone(),
        None => return,
    };
    for desc in &descs {
        if tag(desc.kind) <= 15 || desc.kind == KIND_PTR {
            continue;
        }
        // A fixed-size array field stores its elements inline in the struct
        // allocation (Stage 1): free each element by its own kind instead of
        // treating the field bits as a heap pointer.
        if tag(desc.kind) == KIND_ARRAY {
            let elem_kind = (desc.kind >> 8) & 0xffff_ffff;
            let count = desc.kind >> 40;
            let step = kind_size(elem_kind);
            for i in 0..count as usize {
                let mut buf = 0u64;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        (s as *const u8).add(desc.offset as usize + i * step),
                        &mut buf as *mut u64 as *mut u8,
                        step,
                    );
                }
                free_value(buf, elem_kind);
            }
            continue;
        }
        unsafe {
            let value =
                std::ptr::read_unaligned((s as *const u8).add(desc.offset as usize) as *const u64);
            free_value(value, desc.kind);
        }
    }
    unsafe { libc::free(s as *mut libc::c_void) };
    mark_freed(s);
}

/// Free a fixed-size array: its storage is one `yarrow_alloc` block holding
/// scalar elements, so nothing recurses.
fn free_array(a: u64) {
    if a == 0 || FREED.lock().unwrap().contains(&a) {
        return;
    }
    detach(a);
    unsafe { libc::free(a as *mut libc::c_void) };
    mark_freed(a);
}

/// Free a union block: read the active member's tag, free the inline payload
/// using that member's registered kind code, then free the block itself.
fn free_union(u: u64, id: u32) {
    if u == 0 || FREED.lock().unwrap().contains(&u) {
        return;
    }
    detach(u);
    unsafe {
        let tag = std::ptr::read_unaligned((u as *const u64).add(UNION_TAG_OFFSET as usize));
        let descs = match UNION_DESCS.lock().unwrap().get(&id) {
            Some(d) => d.clone(),
            None => Vec::new(),
        };
        let payload = std::ptr::read_unaligned(
            (u as *const u8).add(UNION_PAYLOAD_OFFSET as usize) as *const u64
        );
        if let Some(kind) = descs.get(tag as usize) {
            free_value(payload, *kind);
        }
        libc::free(u as *mut libc::c_void);
    }
    mark_freed(u);
}

// ---------------------------------------------------------------------------
// Generic free
// ---------------------------------------------------------------------------

/// Free a value and everything it owns, driven by a compiler kind code.
pub extern "C" fn free_value(v: u64, kind: u64) {
    if v == 0 {
        return;
    }
    match tag(kind) {
        0..=15 => {}
        16 => free_str(v),
        KIND_LIST => free_list(v, kind >> 8),
        KIND_MAP => free_map(v, (kind >> 8) & 0xffffffff, kind >> 40),
        KIND_STRUCT => free_struct(v, (kind >> 8) as u32),
        KIND_PTR => {}
        KIND_ARRAY => free_array(v),
        KIND_UNION => free_union(v, (kind >> 8) as u32),
        KIND_ENUM => free_enum(v, (kind >> 8) as u32),
        _ => {}
    }
}

/// Free an enum value through its registered carrier: a string carrier
/// stores a heap string handle; an integer carrier stores inline bits, so
/// there is nothing to free.
fn free_enum(v: u64, id: u32) {
    let carrier = ENUM_DESCS.lock().unwrap().get(&id).copied();
    if carrier.is_some_and(|k| tag(k) == KIND_STRING) {
        free_str(v);
    }
}

/// Register a struct's field layouts so `yarrow_free_value` can free structs.
///
/// # Safety
///
/// `ptr` must point to a valid `FieldDesc` array of at least `count` elements.
pub unsafe extern "C" fn yarrow_register_struct_descs(id: u32, ptr: *const FieldDesc, count: u64) {
    let mut descs = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        descs.push(*ptr.add(i));
    }
    STRUCT_DESCS.lock().unwrap().insert(id, descs);
}

/// Register a union's member kind codes so `yarrow_free_value` can free the
/// union's active payload.
///
/// # Safety
///
/// `ptr` must point to a valid array of at least `count` `u64` kind codes.
pub unsafe extern "C" fn yarrow_register_union_descs(id: u32, ptr: *const u64, count: u64) {
    let mut descs = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        descs.push(*ptr.add(i));
    }
    UNION_DESCS.lock().unwrap().insert(id, descs);
}

/// Register an enum's carrier kind code so `free_value` and
/// `yarrow_value_clone` can handle enum-typed fields and elements.
///
/// # Safety
///
/// Nothing to uphold: the carrier is a plain kind code, not a pointer.
pub unsafe extern "C" fn yarrow_register_enum_carrier(id: u32, carrier: u64) {
    ENUM_DESCS.lock().unwrap().insert(id, carrier);
}

// ---------------------------------------------------------------------------
// Generic clone
// ---------------------------------------------------------------------------

/// Byte size of a value of the given kind, mirroring the compiler's element
/// sizing (heap kinds are 8-byte handles).
fn kind_size(kind: u64) -> usize {
    match tag(kind) {
        0 | 1 | 6 => 1,
        2 | 7 | 12 => 2,
        3 | 8 | 11 | 13 => 4,
        _ => 8,
    }
}

/// Deep-clone a value, driven by the same kind codes as [`free_value`].
/// Scalars and pointers are returned unchanged (their bits are the value);
/// strings, lists, maps, structs, and unions are copied recursively.
///
/// Union payloads clone deeply only when the union's member kinds are
/// registered (see [`yarrow_register_union_descs`]); otherwise the payload
/// handle is shared, matching `free_value`'s treatment of unregistered
/// unions. Fixed arrays share their storage: the runtime does not track their
/// element count, and their elements are scalars.
pub extern "C" fn yarrow_value_clone(v: u64, kind: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    match tag(kind) {
        0..=15 | KIND_PTR => v,
        KIND_STRING => clone_str(v),
        KIND_LIST => clone_list(v, kind >> 8),
        KIND_MAP => clone_map(v, (kind >> 8) & 0xffffffff, kind >> 40),
        KIND_STRUCT => clone_struct(v, (kind >> 8) as u32),
        KIND_ARRAY => v,
        KIND_UNION => clone_union(v, (kind >> 8) as u32),
        KIND_ENUM => clone_enum(v, (kind >> 8) as u32),
        _ => v,
    }
}

/// Clone an enum value through its registered carrier: a string carrier
/// clones its heap string; an integer carrier is scalar bits and clones as
/// itself.
fn clone_enum(v: u64, id: u32) -> u64 {
    let carrier = ENUM_DESCS.lock().unwrap().get(&id).copied();
    if carrier.is_some_and(|k| tag(k) == KIND_STRING) {
        clone_str(v)
    } else {
        v
    }
}

fn clone_str(s: u64) -> u64 {
    unsafe {
        let str = &*(s as *const Str);
        yarrow_str_new(str.ptr as u64, str.len as u64)
    }
}

fn clone_list(l: u64, elem_kind: u64) -> u64 {
    unsafe {
        let list = &*(l as *const List);
        let new = yarrow_list_new(list.elem_size as u64);
        let recurse = tag(elem_kind) > 15 && elem_kind != KIND_PTR;
        for i in 0..list.len {
            let elem = read_elem(list, i);
            let elem = if recurse {
                yarrow_value_clone(elem, elem_kind)
            } else {
                elem
            };
            yarrow_list_push(new, elem);
        }
        new
    }
}

fn clone_map(m: u64, key_kind: u64, val_kind: u64) -> u64 {
    unsafe {
        let map = &*(m as *const Map);
        let new = yarrow_map_new(map.keys_string as u64);
        let clone_keys = tag(key_kind) > 15 && key_kind != KIND_PTR;
        let clone_vals = tag(val_kind) > 15 && val_kind != KIND_PTR;
        for i in 0..map.cap {
            if *map.used.add(i) == 0 {
                continue;
            }
            let key = *map.keys.add(i);
            let val = *map.vals.add(i);
            let key = if clone_keys {
                yarrow_value_clone(key, key_kind)
            } else {
                key
            };
            let val = if clone_vals {
                yarrow_value_clone(val, val_kind)
            } else {
                val
            };
            yarrow_map_insert(new, key, val);
        }
        new
    }
}

/// Byte size of one field of a registered struct descriptor. A fixed-size
/// array field (Stage 1) stores `count` elements inline, so its size is the
/// element size times the count; every other field is one kind-sized value.
fn field_desc_size(kind: u64) -> usize {
    if tag(kind) == KIND_ARRAY {
        let elem_kind = (kind >> 8) & 0xffff_ffff;
        (kind >> 40) as usize * kind_size(elem_kind)
    } else {
        kind_size(kind)
    }
}

/// Clone a struct block: copy the storage byte-for-byte, then replace every
/// heap field with a clone of the original field. The block size is derived
/// from the registered field descriptors (an upper bound of the compiler's
/// aligned layout), so a missing registration leaves the block shared rather
/// than guessing a size.
fn clone_struct(s: u64, id: u32) -> u64 {
    let descs = match STRUCT_DESCS.lock().unwrap().get(&id) {
        Some(d) => d.clone(),
        None => return s,
    };
    let mut size = 0usize;
    for d in &descs {
        size = size.max(d.offset as usize + field_desc_size(d.kind));
    }
    size = size.max(1);
    let new = unsafe { libc::malloc(size) } as u64;
    unsafe { std::ptr::copy_nonoverlapping(s as *const u8, new as *mut u8, size) };
    for d in &descs {
        if tag(d.kind) <= 15 || d.kind == KIND_PTR {
            continue;
        }
        // A fixed-size array field stores its elements inline (Stage 1):
        // clone each heap element in place instead of treating the field
        // bits as one heap pointer.
        if tag(d.kind) == KIND_ARRAY {
            let elem_kind = (d.kind >> 8) & 0xffff_ffff;
            let count = (d.kind >> 40) as usize;
            let step = kind_size(elem_kind);
            if tag(elem_kind) <= 15 {
                continue;
            }
            for i in 0..count {
                let mut buf = 0u64;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        (s as *const u8).add(d.offset as usize + i * step),
                        &mut buf as *mut u64 as *mut u8,
                        step,
                    );
                    let cloned = yarrow_value_clone(buf, elem_kind);
                    std::ptr::copy_nonoverlapping(
                        &cloned as *const u64 as *const u8,
                        (new as *mut u8).add(d.offset as usize + i * step),
                        step,
                    );
                }
            }
            continue;
        }
        unsafe {
            let field =
                std::ptr::read_unaligned((s as *const u8).add(d.offset as usize) as *const u64);
            let cloned = yarrow_value_clone(field, d.kind);
            std::ptr::write_unaligned((new as *mut u8).add(d.offset as usize) as *mut u64, cloned);
        }
    }
    new
}

/// Clone a union block: copy the active-member tag, then clone the payload
/// with the registered member kind (shared when no kind is registered).
fn clone_union(u: u64, id: u32) -> u64 {
    unsafe {
        let active =
            std::ptr::read_unaligned((u as *const u8).add(UNION_TAG_OFFSET as usize) as *const u64);
        let payload = std::ptr::read_unaligned(
            (u as *const u8).add(UNION_PAYLOAD_OFFSET as usize) as *const u64
        );
        let member_kind = UNION_DESCS
            .lock()
            .unwrap()
            .get(&id)
            .and_then(|d| d.get(active as usize).copied());
        let payload = match member_kind {
            Some(k) if tag(k) > 15 && k != KIND_PTR => yarrow_value_clone(payload, k),
            _ => payload,
        };
        let new = yarrow_alloc(16);
        std::ptr::write_unaligned(
            (new as *mut u8).add(UNION_TAG_OFFSET as usize) as *mut u64,
            active,
        );
        std::ptr::write_unaligned(
            (new as *mut u8).add(UNION_PAYLOAD_OFFSET as usize) as *mut u64,
            payload,
        );
        new
    }
}

// ---------------------------------------------------------------------------
// Regions
// ---------------------------------------------------------------------------

/// Create a new heap region; returns a handle.
pub extern "C" fn yarrow_region_new() -> u64 {
    let hdr = unsafe { libc::malloc(std::mem::size_of::<Region>()) } as *mut Region;
    unsafe {
        *hdr = Region {
            cap: 0,
            len: 0,
            values: std::ptr::null_mut(),
        };
    }
    hdr as u64
}

/// Attach `value` (with its kind code) to `region`. Nested values are freed
/// through their parent, so only the top-level handle is registered.
pub extern "C" fn yarrow_region_register(value: u64, kind: u64, region: u64) {
    if region == 0 || tag(kind) <= 15 {
        return;
    }
    detach(value);
    unsafe {
        let reg = &mut *(region as *mut Region);
        if reg.len == reg.cap {
            let new_cap = if reg.cap == 0 { 8 } else { reg.cap * 2 };
            reg.values = libc::realloc(
                reg.values as *mut libc::c_void,
                new_cap * std::mem::size_of::<RegionEntry>(),
            ) as *mut RegionEntry;
            reg.cap = new_cap;
        }
        *reg.values.add(reg.len) = RegionEntry { value, kind };
        reg.len += 1;
    }
    REGISTRY.lock().unwrap().insert(value, region);
}

/// Free every value attached to `region`, then the region itself.
pub extern "C" fn yarrow_region_free(region: u64) {
    if region == 0 || FREED.lock().unwrap().contains(&region) {
        return;
    }
    unsafe {
        let reg = &mut *(region as *mut Region);
        for i in 0..reg.len {
            let entry = *reg.values.add(i);
            // Remove from the registry before freeing so a later free of the
            // same handle (e.g. a variable drop at scope exit) is a no-op.
            REGISTRY.lock().unwrap().remove(&entry.value);
            free_value(entry.value, entry.kind);
        }
        libc::free(reg.values as *mut libc::c_void);
        libc::free(region as *mut libc::c_void);
    }
    mark_freed(region);
}

// ---------------------------------------------------------------------------
// Printing (used by the '@print...' builtins)
// ---------------------------------------------------------------------------

pub extern "C" fn yarrow_print_str(s: u64) {
    use std::io::Write;
    unsafe {
        let str = &*(s as *const Str);
        let bytes = std::slice::from_raw_parts(str.ptr, str.len);
        let _ = std::io::stdout().lock().write_all(bytes);
        let _ = std::io::stdout().lock().flush();
    }
}

pub extern "C" fn yarrow_print_int(v: i64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(v.to_string().as_bytes());
    let _ = out.flush();
}

pub extern "C" fn yarrow_print_float(v: f64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(v.to_string().as_bytes());
    let _ = out.flush();
}

pub extern "C" fn yarrow_print_newline() {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

// ---------------------------------------------------------------------------
// Container printing (@print_array / @print_list / @print_hashmap)
// ---------------------------------------------------------------------------

/// Byte width of a scalar kind code (`scalar_code` encoding, plus the string
/// and pointer handles at 8 bytes).
fn scalar_bits(code: u64) -> u64 {
    match code {
        0 | 1 | 6 => 8,
        2 | 7 | 12 => 16,
        3 | 8 | 11 | 13 => 32,
        _ => 64,
    }
}

/// Read a scalar of `bits` width from `p` (used for fixed-size arrays, whose
/// storage is one contiguous block).
unsafe fn read_scalar(p: *const u8, bits: u64) -> u64 {
    unsafe {
        match bits {
            8 => *p as u64,
            16 => *(p as *const u16) as u64,
            32 => *(p as *const u32) as u64,
            _ => *(p as *const u64),
        }
    }
}

fn print_scalar(out: &mut dyn std::io::Write, v: u64, code: u64) {
    match code {
        0 => {
            let _ = write!(out, "{}", v != 0);
        }
        1 => {
            let _ = write!(out, "{}", v as i8);
        }
        2 => {
            let _ = write!(out, "{}", v as i16);
        }
        3 => {
            let _ = write!(out, "{}", v as i32);
        }
        4 => {
            let _ = write!(out, "{}", v as i64);
        }
        6 => {
            let _ = write!(out, "{}", v as u8);
        }
        7 => {
            let _ = write!(out, "{}", v as u16);
        }
        8 => {
            let _ = write!(out, "{}", v as u32);
        }
        9 => {
            let _ = write!(out, "{}", v);
        }
        11 => match char::from_u32(v as u32) {
            Some(c) => {
                let _ = write!(out, "{c}");
            }
            None => {
                let _ = write!(out, "<invalid rune>");
            }
        },
        13 => {
            let _ = write!(out, "{}", f32::from_bits(v as u32));
        }
        14 => {
            let _ = write!(out, "{}", f64::from_bits(v));
        }
        _ => {
            let _ = write!(out, "{}", v as i64);
        }
    }
}

unsafe fn print_str_to(out: &mut dyn std::io::Write, s: u64) {
    if s == 0 {
        return;
    }
    let str = unsafe { &*(s as *const Str) };
    let bytes = unsafe { std::slice::from_raw_parts(str.ptr, str.len) };
    let _ = out.write_all(bytes);
}

/// Print one value by its runtime kind code. Scalars/strings/containers print
/// their contents; structs, unions and generic pointers have no runtime field
/// names or element kind, so they degrade to a descriptor.
unsafe fn print_value_to(out: &mut dyn std::io::Write, v: u64, kind: u64) {
    match tag(kind) {
        0..=15 => print_scalar(out, v, kind),
        16 => unsafe { print_str_to(out, v) },
        0x20 => unsafe { print_list_to(out, v, kind >> 8) },
        0x30 => unsafe { print_map_to(out, v, (kind >> 8) & 0xffffffff, kind >> 40) },
        0x60 => unsafe { print_array_to(out, v, kind >> 8, kind >> 40) },
        0x40 => {
            let _ = write!(out, "#<struct {}>", kind >> 8);
        }
        0x50 => {
            let _ = write!(out, "#<ptr {v:#x}>");
        }
        0x70 => {
            let _ = write!(out, "#<union {}>", kind >> 8);
        }
        _ => {
            let _ = write!(out, "{v}");
        }
    }
}

unsafe fn print_list_to(out: &mut dyn std::io::Write, l: u64, elem_kind: u64) {
    if l == 0 {
        let _ = write!(out, "()");
        return;
    }
    let list = unsafe { &*(l as *const List) };
    let _ = write!(out, "(");
    for i in 0..list.len {
        if i > 0 {
            let _ = write!(out, ", ");
        }
        let elem = read_elem(list, i);
        unsafe { print_value_to(out, elem, elem_kind) };
    }
    let _ = write!(out, ")");
}

unsafe fn print_map_to(out: &mut dyn std::io::Write, m: u64, key_kind: u64, val_kind: u64) {
    if m == 0 {
        let _ = write!(out, "{{}}");
        return;
    }
    let map = unsafe { &*(m as *const Map) };
    let _ = write!(out, "{{");
    let mut first = true;
    for i in 0..map.cap {
        if unsafe { *map.used.add(i) } == 0 {
            continue;
        }
        if !first {
            let _ = write!(out, ", ");
        }
        let key = unsafe { *map.keys.add(i) };
        let val = unsafe { *map.vals.add(i) };
        unsafe { print_value_to(out, key, key_kind) };
        let _ = write!(out, ": ");
        unsafe { print_value_to(out, val, val_kind) };
        first = false;
    }
    let _ = write!(out, "}}");
}

unsafe fn print_array_to(out: &mut dyn std::io::Write, a: u64, elem_kind: u64, count: u64) {
    if a == 0 {
        let _ = write!(out, "[]");
        return;
    }
    let bits = scalar_bits(elem_kind);
    let step = (bits / 8) as usize;
    let _ = write!(out, "[");
    for i in 0..count {
        if i > 0 {
            let _ = write!(out, ", ");
        }
        let elem = read_scalar((a as *const u8).add(i as usize * step), bits);
        unsafe { print_value_to(out, elem, elem_kind) };
    }
    let _ = write!(out, "]");
}

pub extern "C" fn yarrow_print_array(a: u64, kind: u64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    unsafe { print_array_to(&mut out, a, kind >> 8, kind >> 40) };
    let _ = out.flush();
}

pub extern "C" fn yarrow_print_list(l: u64, kind: u64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    unsafe { print_list_to(&mut out, l, kind >> 8) };
    let _ = out.flush();
}

pub extern "C" fn yarrow_print_hashmap(m: u64, kind: u64) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    unsafe { print_map_to(&mut out, m, (kind >> 8) & 0xffffffff, kind >> 40) };
    let _ = out.flush();
}

// ---------------------------------------------------------------------------
// Filesystem (`std.fs`)
// ---------------------------------------------------------------------------

/// Host error codes for `fs_*` (also negated from `fs_open` on failure).
pub const FS_ERR_IO: i64 = 1;
pub const FS_ERR_NOT_FOUND: i64 = 2;
pub const FS_ERR_INVALID: i64 = 3;

thread_local! {
    static FS_LAST_ERROR: Cell<i64> = const { Cell::new(0) };
}

fn fs_set_error(code: i64) {
    FS_LAST_ERROR.with(|c| c.set(code));
}

fn fs_map_io_error(err: &std::io::Error) -> i64 {
    match err.kind() {
        std::io::ErrorKind::NotFound => FS_ERR_NOT_FOUND,
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => FS_ERR_INVALID,
        _ => FS_ERR_IO,
    }
}

fn fs_path_from_handle(s: u64) -> Result<std::path::PathBuf, i64> {
    if s == 0 {
        return Err(FS_ERR_INVALID);
    }
    unsafe {
        let str = &*(s as *const Str);
        let bytes = std::slice::from_raw_parts(str.ptr, str.len);
        match std::str::from_utf8(bytes) {
            Ok(p) => Ok(std::path::PathBuf::from(p)),
            Err(_) => Err(FS_ERR_INVALID),
        }
    }
}

fn fs_empty_string() -> u64 {
    yarrow_str_new(0, 0)
}

/// Open `path` with mode rune `'r'` / `'w'` / `'a'`.
/// Returns a non-negative fd on success, or `-FS_ERR_*` on failure.
pub extern "C" fn yarrow_fs_open(path: u64, mode: i64) -> i64 {
    fs_set_error(0);
    let path = match fs_path_from_handle(path) {
        Ok(p) => p,
        Err(code) => {
            fs_set_error(code);
            return -code;
        }
    };
    let mut opts = std::fs::OpenOptions::new();
    match mode as u32 {
        // 'r'
        0x72 => {
            opts.read(true);
        }
        // 'w'
        0x77 => {
            opts.write(true).create(true).truncate(true);
        }
        // 'a'
        0x61 => {
            opts.write(true).create(true).append(true);
        }
        _ => {
            fs_set_error(FS_ERR_INVALID);
            return -FS_ERR_INVALID;
        }
    };
    match opts.open(&path) {
        Ok(file) => {
            #[cfg(unix)]
            {
                use std::os::fd::IntoRawFd;
                file.into_raw_fd() as i64
            }
            #[cfg(windows)]
            {
                use std::os::windows::io::IntoRawHandle;
                file.into_raw_handle() as isize as i64
            }
        }
        Err(err) => {
            let code = fs_map_io_error(&err);
            fs_set_error(code);
            -code
        }
    }
}

/// Close a host fd / handle. Returns `0` on success or `FS_ERR_*`. Negative /
/// invalid values are ignored so failed-open fallbacks (`fd -1`) are safe to
/// close.
pub extern "C" fn yarrow_fs_close(fd: i64) -> i64 {
    fs_set_error(0);
    if fd < 0 {
        return 0;
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::close(fd as libc::c_int) };
        if rc == 0 {
            0
        } else {
            let code = FS_ERR_IO;
            fs_set_error(code);
            code
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{FromRawHandle, RawHandle};
        // Dropping the File closes the HANDLE.
        drop(unsafe { std::fs::File::from_raw_handle(fd as isize as RawHandle) });
        0
    }
}

/// Write all bytes of string handle `s` to `fd`. Returns `0` or `FS_ERR_*`.
pub extern "C" fn yarrow_fs_write(fd: i64, s: u64) -> i64 {
    use std::io::Write;

    fs_set_error(0);
    if fd < 0 {
        fs_set_error(FS_ERR_INVALID);
        return FS_ERR_INVALID;
    }
    if s == 0 {
        fs_set_error(FS_ERR_INVALID);
        return FS_ERR_INVALID;
    }
    let bytes = unsafe {
        let str = &*(s as *const Str);
        std::slice::from_raw_parts(str.ptr, str.len)
    };
    let result = fs_with_file(fd, |file| file.write_all(bytes).and_then(|_| file.flush()));
    match result {
        Ok(()) => 0,
        Err(err) => {
            let code = fs_map_io_error(&err);
            fs_set_error(code);
            code
        }
    }
}

/// Read the remainder of `fd` into a new string handle. On failure returns an
/// empty string and sets [`yarrow_fs_last_error`].
pub extern "C" fn yarrow_fs_read(fd: i64) -> u64 {
    use std::io::Read;

    fs_set_error(0);
    if fd < 0 {
        fs_set_error(FS_ERR_INVALID);
        return fs_empty_string();
    }
    let mut buf = Vec::new();
    let result = fs_with_file(fd, |file| file.read_to_end(&mut buf));
    match result {
        Ok(_) => {
            if buf.is_empty() {
                fs_empty_string()
            } else {
                yarrow_str_new(buf.as_ptr() as u64, buf.len() as u64)
            }
        }
        Err(err) => {
            let code = fs_map_io_error(&err);
            fs_set_error(code);
            fs_empty_string()
        }
    }
}

/// Borrow a host file descriptor / handle as a [`std::fs::File`] without
/// closing it when the borrow ends.
fn fs_with_file<T, F>(fd: i64, f: F) -> std::io::Result<T>
where
    F: FnOnce(&mut std::fs::File) -> std::io::Result<T>,
{
    #[cfg(unix)]
    {
        use std::os::fd::{FromRawFd, IntoRawFd};
        let mut file = unsafe { std::fs::File::from_raw_fd(fd as libc::c_int) };
        let result = f(&mut file);
        let _ = file.into_raw_fd();
        result
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::{FromRawHandle, IntoRawHandle, RawHandle};
        let mut file = unsafe { std::fs::File::from_raw_handle(fd as isize as RawHandle) };
        let result = f(&mut file);
        let _ = file.into_raw_handle();
        result
    }
}

/// Last `FS_ERR_*` from a failed `fs_*` call (`0` after success).
pub extern "C" fn yarrow_fs_last_error() -> i64 {
    FS_LAST_ERROR.with(|c| c.get())
}

// ---------------------------------------------------------------------------
// Host function registry
// ---------------------------------------------------------------------------

/// Scalar kind codes used in host-function signatures; they match
/// `scalar_code`/`scalar_ty` in the compiler (and the runtime `tag()` cut-off
/// at 16 keeps them distinct from heap value kinds).
pub const KIND_I64: u64 = 4;
pub const KIND_F64: u64 = 14;

/// Whether a host function is only callable from an unsafe context (an
/// `unsafe` block or an unsafe function). Safe functions are always callable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    Safe,
    Unsafe,
}

/// A host function callable from Yarrow: its C ABI signature encoded with
/// scalar kind codes plus the symbol's address. The compiler resolves `@name`
/// words and calls to undefined functions against this table, so adding a host
/// function needs no compiler changes (the generic lowering reads the
/// signature here).
pub struct HostFn {
    pub name: &'static str,
    pub params: &'static [u64],
    pub returns: &'static [u64],
    pub address: usize,
    /// Whether calling this function requires an unsafe context.
    pub safety: Safety,
}

/// The tiny host surface: raw memory only (`alloc`/`free`) plus the scalar
/// helpers the compiler still inlines around until Stage 5 moves them into
/// Yarrow's std library. Signatures are all 64-bit. Built lazily because the
/// symbol addresses are only known at runtime.
pub static HOST_FNS: std::sync::LazyLock<Vec<HostFn>> = std::sync::LazyLock::new(|| {
    vec![
        HostFn {
            name: "alloc",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_alloc as *const () as usize,
            safety: Safety::Unsafe,
        },
        HostFn {
            name: "free",
            params: &[KIND_I64],
            returns: &[],
            address: yarrow_free as *const () as usize,
            safety: Safety::Unsafe,
        },
        HostFn {
            name: "str_new",
            params: &[KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_str_new as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "str_len",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_str_len as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "str_join",
            params: &[KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_str_join as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "str_cmp",
            params: &[KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_str_cmp as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "list_new",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_list_new as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "list_len",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_list_len as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "list_get",
            params: &[KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_list_get as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "list_push",
            params: &[KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_list_push as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "list_set",
            params: &[KIND_I64, KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_list_set as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "map_new",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_map_new as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "map_insert",
            params: &[KIND_I64, KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_map_insert as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "map_get",
            params: &[KIND_I64, KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_map_get as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "map_len",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_map_len as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_str",
            params: &[KIND_I64],
            returns: &[],
            address: yarrow_print_str as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_int",
            params: &[KIND_I64],
            returns: &[],
            address: yarrow_print_int as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_float",
            params: &[KIND_F64],
            returns: &[],
            address: yarrow_print_float as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_newline",
            params: &[],
            returns: &[],
            address: yarrow_print_newline as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_array",
            params: &[KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_print_array as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_list",
            params: &[KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_print_list as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "print_hashmap",
            params: &[KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_print_hashmap as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "free_value",
            params: &[KIND_I64, KIND_I64],
            returns: &[],
            address: free_value as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "register_struct_descs",
            params: &[KIND_I64, KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_register_struct_descs as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "register_union_descs",
            params: &[KIND_I64, KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_register_union_descs as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "register_enum_carrier",
            params: &[KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_register_enum_carrier as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "region_new",
            params: &[],
            returns: &[KIND_I64],
            address: yarrow_region_new as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "region_register",
            params: &[KIND_I64, KIND_I64, KIND_I64],
            returns: &[],
            address: yarrow_region_register as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "region_free",
            params: &[KIND_I64],
            returns: &[],
            address: yarrow_region_free as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "fs_open",
            params: &[KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_fs_open as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "fs_close",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_fs_close as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "fs_write",
            params: &[KIND_I64, KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_fs_write as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "fs_read",
            params: &[KIND_I64],
            returns: &[KIND_I64],
            address: yarrow_fs_read as *const () as usize,
            safety: Safety::Safe,
        },
        HostFn {
            name: "fs_last_error",
            params: &[],
            returns: &[KIND_I64],
            address: yarrow_fs_last_error as *const () as usize,
            safety: Safety::Safe,
        },
    ]
});
