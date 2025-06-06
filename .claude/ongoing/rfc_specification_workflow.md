# RFC and Specification Workflow Design

## Status
Planning phase - All major design decisions resolved, ready for implementation

## Context
We want to establish a clear RFC and specification workflow for Dada language development. The goal is to:
- Make RFCs and specifications visible and accessible
- Use lightweight, focused tools (mdbook over Docusaurus)
- Separate concerns between RFCs, specs, and user documentation

## Decisions Made So Far

### Storage Structure
- `rfcs/` - mdbook for RFCs, deployed to `dada-lang.org/rfcs`
- `spec/` - mdbook for language specification, deployed to `dada-lang.org/spec` 
- `book/` - existing Docusaurus for tutorials/user docs at `dada-lang.org`
- Update `cargo xtask deploy` to handle all three sites

### Rationale
- mdbook is lighter weight and more approachable than Docusaurus
- RFCs and specs are distinct enough to warrant separation
- Keeps implementation docs (rustdoc) separate from language docs

## Resolved Decisions

1. **RFC lifecycle** - RFCs remain in the rfcs mdbook permanently as historical records, organized by status (Active, Accepted, Implemented, Rejected/Withdrawn)

2. **RFC numbering** - Simple sequential numbering (0001, 0002, etc.) with topic-based organization in SUMMARY.md

3. **RFC directory structure**:
   ```
   rfcs/
     src/
       SUMMARY.md
       0001-feature-name/
         README.md           # The main RFC document
         implementation.md   # Implementation progress tracking
         spec-draft.md      # Draft spec text (staging ground)
         examples/          # Example code if needed
   ```

4. **RFC to spec flow** - Mix of manual integration with spec drafts in RFC subdirectories:
   - RFCs can include draft spec text in `spec-draft.md` as a staging ground
   - When RFC is implemented, spec text is manually integrated into the main spec
   - Spec includes non-normative references to RFCs for motivation/context
   - Spec remains authoritative and factual; RFCs provide teaching/motivation

## Resolved Decisions (continued)

5. **RFC template**:
   ```markdown
   # RFC-0001: Title

   ## Summary
   Brief one-paragraph explanation

   ## Motivation
   Why are we doing this? What use cases does it support?

   ## Guide-level explanation
   Explain the proposal as if teaching it to another Dada programmer

   ## Reference-level explanation
   Technical details and edge cases

   ## Frequently asked questions
   Common questions and concerns about this design

   ## Future possibilities
   What future extensions or changes might this enable?
   ```

## Resolved Decisions (continued)

6. **Implementation tracking** - Combination approach:
   - SUMMARY.md organized by status sections (Active, Accepted, Implemented)
   - Each RFC README.md includes front matter with status:
     ```markdown
     ---
     status: active|accepted|implemented|rejected
     tracking-issue: #123  # optional
     implemented-version: 0.1.0  # optional, for implemented RFCs
     ---
     ```
   - Detailed implementation progress tracked in `implementation.md` within RFC directory

## Open Questions to Resolve

1. **Spec organization** - ✅ RESOLVED: Topic-based organization with Ferrocene-style paragraph identifiers
   - **Structure**: Organized by topic (syntax, type system, permissions, async, etc.) similar to Rust Reference
   - **Paragraph identifiers**: Each paragraph has a unique semantic identifier for cross-linking with tests
   - **Ferrocene inspiration**: Similar to Ferrocene Language Specification (influenced by Ada Reference Manual)
     - Each paragraph specifies one independent, testable rule/behavior  
     - Enables precise cross-referencing and requirement traceability
     - Format: `chapter:section:paragraph` using semantic names instead of numbers
     - Examples: `syntax:string-literals:escape-sequences`, `permissions:lease:transfer-rules`, `types:classes:field-access`
   - **Rationale for semantic IDs**: More stable during spec evolution and reorganization than numeric identifiers

2. **Cross-linking** - PARTIALLY RESOLVED
   - **Paragraph labeling**: Use `r[semantic.id]` syntax (like Rust Reference) to label spec paragraphs
   - **Test linking**: Tests annotated with `#:spec foo.bar.baz` to indicate which spec paragraph they test
   - **Markdown linking**: Still deciding between:
     a) Start with `/spec/...` URLs (works in production, broken locally)
     b) Add preprocessor later for environment-aware links
     c) Other options discussed: full URLs, link mapping file, shorthand syntax

3. **Decision authority** - ✅ RESOLVED: nikomatsakis acts as BDFL for RFC acceptance decisions

## Progress Update (2025-06-02)

### Recently Completed
- ✅ **mdbook structures** - Both `rfcs/` and `spec/` mdbooks created and working
- ✅ **Deploy script updated** - `cargo xtask deploy` now builds all three sites (Docusaurus, RFC mdbook, spec mdbook, rustdocs)
- ✅ **Paragraph labeling implemented** - `r[...]` syntax working in spec files
- ✅ **Markdown preprocessor created** - `dada-mdbook-preprocessor` processes `r[...]` labels into styled HTML anchors
- ✅ **Preprocessor applied to both books** - Both RFC and spec books use the preprocessor

### Current Task: CSS Consolidation ✅ COMPLETED

**Solution Implemented**: 
- Discovered that mdbook preprocessors cannot modify configuration dynamically
- Implemented inline CSS injection approach instead
- Preprocessor now:
  1. Scans each chapter for `r[...]` labels
  2. Processes labels into HTML with proper anchors
  3. Injects `<style>` tags at the end of chapters that contain labels
- Removed manual `additional-css` entries from both book.toml files
- Tested with both RFC and spec books - working correctly

### Recent Progress (2025-06-03)

**✅ RFC Template and Workflow Completed**
- Created RFC-0000 template as concise skeleton in RFC mdbook
- Implemented `cargo xtask rfc new` command that:
  - Auto-finds next RFC number
  - Creates RFC directory from template
  - Copies all template files with placeholder replacement
  - Updates SUMMARY.md automatically
- Updated `.development/rfc.md` workflow documentation
- Removed duplicate content and updated structure

### Next Steps
- Modify RFC preprocessor to auto-populate SUMMARY.md based on existing directories
- Set up test annotation system with `#:spec` comments
- Decide on markdown linking strategy
- Create example RFC using the new infrastructure