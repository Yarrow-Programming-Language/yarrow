# Hashmap operations. `len`, `get`, and `put` are compiler intrinsics
# resolved when this module is required. They work for any `hashmap<K V>`
# key/value types supported by the host map runtime.
#
# `get` raises an `E372` diagnostic when the key is absent; there is no
# default value. `put` overwrites the value when the key already exists.
