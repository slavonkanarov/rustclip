# Agent instructions

## Project status

- This repository is in the planning stage. It currently contains documentation only; do not describe planned features as implemented.
- [docs/requirements.md](docs/requirements.md) is the source of product requirements. [docs/technical-notes.md](docs/technical-notes.md) contains implementation guidance, not approved library choices.
- Do not create a Cargo project or application code without a separate implementation request. This describes the current stage and does not restrict future work requested by the owner.

## Language policy

- Write all project artifacts in English: UI copy, settings, notifications, errors, documentation, code comments, test names, commit and PR text, and other text intended for users or developers.
- Communicate with the project owner in Russian. Do not translate project files into Russian to match the conversation language.

## Future implementation

- Prioritize responsiveness, stability, and clipboard data integrity. Do not block keyboard handling on image reads, disk writes, or search.
- Distinguish external copies from writes made when selecting an old entry. Selection must not add a history entry; a later external copy of identical content is still a new event.
- Handle errors and unknown formats. Text and images are required; a malformed format must not crash the process or prevent other formats from being processed.
- Capture the active window before showing rustclip UI. Isolate clipboard, global shortcut, window, and paste operations behind Windows-specific interfaces.
- Make every keyboard command configurable, including navigation in the quick preview and main window. Detect conflicts and report global shortcut registration failures.
- Enforce the configured storage limit, including images. Pruning old entries must not block the UI.
- Keep clipboard contents and window titles out of diagnostic logs. Store history locally.

## Making changes

- Update requirements, acceptance scenarios, and technical notes when behavior changes.
- Check rapid copies, self-initiated clipboard writes, identical content copied externally, large images, malformed formats, and closure of the original window.
- Preserve unrelated files and user changes. At this stage, check documentation links and consistency; there are no build commands yet.
