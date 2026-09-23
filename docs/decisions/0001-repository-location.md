# Decision: repository location

Date: 2026-09-23
Status: accepted

## Context

The bootstrap brief says to use an existing Avdium repository if the task was
started inside one, otherwise to create `avdium/` beneath the user-selected
workspace, and not to nest a repository accidentally.

The session started in `$HOME/Desktop/Projects/Avdium`: an empty
directory that is not a Git repository, already named for the project.

## Decision

Initialize the repository directly in `$HOME/Desktop/Projects/Avdium`.

## Rationale

The directory is already the dedicated, empty workspace for this project.
Creating `Avdium/avdium/` would be the accidental nesting the brief warns
against, and would leave the selected directory as an untracked wrapper.

## Consequences

The workspace root is the repository root. No other project lives here.
