# Schema Metadata

- **Schema Name:** Yarrow Plan Schema
- **Version:** 1.0.0
- **Last Updated:** 2026-11-01
- **Author:** Yarrow Project
- **Description:** Schema for Yarrow crate implementation plans. Defines the structure for tracking stages, features, and implementation status across all Yarrow crates.

---

## Version History

- **1.0.0** (2026-11-01): Initial version based on plan.md format

---

## Custom Data Types

### Predefined Lists

#### Crate Name

- **`Values`:** yarrow-core, yarrow-cli, yarrow-fmt, yarrow-lsp, yarrow-runtime
- **`MD-representations`:**
  - `yarrow-core`: Yarrow Core
  - `yarrow-cli`: Yarrow CLI
  - `yarrow-fmt`: Yarrow Formatter
  - `yarrow-lsp`: Yarrow LSP
  - `yarrow-runtime`: Yarrow Runtime

#### Stage Status

- **`Values`:** pending, in_progress, landed, blocked, deferred, canceled
- **`Progressions`:**
  - pending -> in_progress
  - in_progress -> landed
  - pending -> blocked
  - blocked -> in_progress
  - in_progress -> deferred
  - deferred -> in_progress
  - in_progress -> canceled
- **`MD-representations`:**
  - `pending`: Pending
  - `in_progress`: In Progress
  - `landed`: Landed
  - `blocked`: Blocked
  - `deferred`: Deferred
  - `canceled`: Canceled

#### Phase

- **`Values`:** A, B, C, D, E, F, G, H, I, J
- **`MD-representations`:**
  - `A`: Phase A
  - `B`: Phase B
  - `C`: Phase C
  - `D`: Phase D
  - `E`: Phase E
  - `F`: Phase F
  - `G`: Phase G
  - `H`: Phase H
  - `I`: Phase I
  - `J`: Phase J

#### Priority

- **`Values`:** high, medium, low
- **`MD-representations`:**
  - `high`: High
  - `medium`: Medium
  - `low`: Low

### Objects

#### SourceOfTruth

- **`Description`:** Reference documentation for the plan
- **`Path`:** String (required)
- **`Type`:** String (optional) - e.g., "Language Spec", "AST Docs", "Style Guide"

#### Dependency

- **`Crate`:** Crate Name (required)
- **`Stage`:** Number (required)
- **`Description`:** String (optional)

#### PlatformTarget

- **`Name`:** String (required) - e.g., "linux-gnu", "windows-msvc", "apple-darwin"
- **`Architectures`:** List of String (required)
- **`Status`:** Stage Status (required)
- **`Notes`:** String (optional)

#### Feature

- **`Name`:** String (required)
- **`Description`:** String (required)
- **`Status`:** Stage Status (required)
- **`Since`:** String (optional) - version or date when landed

---

## Plan Schema

### Plan

- **Name:** String (required) - The crate name
- **Description:** String (required) - Brief description of the crate purpose
- **Version:** String (optional) - Schema version used
- **Last Updated:** Date (YYYY-MM-DD) (optional)
- **Sources of Truth:** `List of` SourceOfTruth (required)
- **Dependencies:** `List of` Dependency (optional)
- **Features:** `List of` Feature (optional)
- **Phases:** `List of` Phase (required)
- **Platform Targets:** `List of` PlatformTarget (optional)
- **Next:** String (optional) - Next focus areas
- **Backlog:** `List of` Stage (optional)
- **Working Rules:** `List of` String (required)

### Phase

- **Name:** Phase (required)
- **Description:** String (required)
- **Status:** Stage Status (required)
- **Stages:** `List of` Stage (required)

### Stage

- **Number:** Number (required) (unique within plan)
- **Name:** String (required)
- **Description:** String (required)
- **Status:** Stage Status (required)
- **Priority:** Priority (optional)
- **Tasks:** `List of` Task (required)
- **Gate:** String (required) - Completion criteria
- **Notes:** String (optional)

### Task

- **Description:** String (required)
- **Status:** Stage Status (optional, defaults to pending)
- **Details:** String (optional)

### Feature

- **Name:** String (required)
- **Description:** String (required)
- **Status:** Stage Status (required)
- **Category:** String (optional)

---

## Plan Structure

Each crate plan file must contain:

1. **Overview** section with Plan metadata (Name, Description, Sources of Truth)
2. **Current State** section with:
   - Landed Features (features with status: landed)
   - Known Gaps (features with status: pending, in_progress, blocked, or deferred)
3. **Phases** section with `List of` Phase objects
4. **Platform Support** section (if applicable) with `List of` PlatformTarget
5. **Backlog** section with upcoming stages
6. **Working Rules** section with implementation guidelines

Optional sections:

- **Next** - Next focus areas
- **Mapping** - Mapping between features and stages

---

## Usage Notes

- **No Due Dates:** As per project requirements, do not include due dates or time estimates
- **Status Updates:** When a stage is completed, update its status to "landed" and move it to the appropriate section
- **Phase Organization:** Group stages into phases for better organization
- **Cross-References:** Use `See: <crate> Stage <number>` for dependencies between crates
- **Gate Criteria:** Each stage must have clear, testable gate criteria
- **Markdown Format:** Use standard Markdown syntax for formatting
