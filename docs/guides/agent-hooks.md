# Agent hooks

The agent hooks are optional. The index, `map` and every query verb work without them; they only
change what a coding agent sees when it reads a file it has already read in the same session.

## What `init` installs

`devscout init` merges two `PostToolUse` entries into the agent settings file at
`$HOME/.claude/settings.json`, backing up the existing file first: `devscout hook read` and
`devscout hook bash`. They read a tool result on stdin and, when the same content has already
been read in the session, replace the payload with a one-line marker instead of repeating it.
Skip this with `devscout init --no-hooks`; the hook install is independent of the index, and a
failure there never fails `init`.

The two entries match the `Read` and `Bash` tools and run the absolute path of the `devscout`
binary that ran `init`:

```json
{
  "hooks": {
    "PostToolUse": [
      { "matcher": "Read", "hooks": [{ "type": "command", "command": "/path/to/devscout hook read" }] },
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "/path/to/devscout hook bash" }] }
    ]
  }
}
```

`init` prints what it did on its `hooks:` line:

- `hooks: installed (Read, Bash); backup <path>`: the entries were added and the previous file
  was copied to `settings.json.bak.<timestamp>` beside it. Every other key in the file is kept.
- `hooks: already installed (idempotent)`: both entries were already present, so nothing was
  written and no second backup was made.
- `hooks: no settings file at <path> -- add manually:` (or a settings file that is not valid
  JSON, or has an unexpected shape): nothing was written, and `init` prints the snippet above
  with the real binary path for you to merge by hand.
- `hooks: skipped (--no-hooks)`.

## What the read hook offers

On the first agent-hook read of an indexed code file, devscout offers the
nearest mapped symbol. A ranged read chooses the declaration nearest to the
requested range; a full-file read chooses the first mapped declaration. The
offer does not replace or truncate the file content, and non-code, unmapped,
stale, or already-offered reads do not produce another offer.

## What the hooks store

The hooks are the only writers of the read-freshness cache, `<root>/.scout/cache.db`, and of the
content-addressed `content.db`; plain CLI use does not touch them. `devscout clear` drops
freshness rows by age or by session, and `devscout stats` summarises them. Where these files live,
and the variables that move them, are in [Storage](../reference/storage.md) and
[Environment variables](../reference/environment.md). `SCOUT_DEBUG=1`, or a `.scout/debug`
file, turns on hook debug output.

## Remove the hooks

1. Open `$HOME/.claude/settings.json`.
2. Under `hooks.PostToolUse`, delete the two entries whose `command` ends in `devscout hook read`
   and `devscout hook bash`. If `PostToolUse` is then empty, delete it too.
3. Or restore the backup `init` printed: copy `settings.json.bak.<timestamp>` over
   `settings.json`. This also drops any change made to the file since the backup.
4. Optionally delete the read-freshness cache, `<root>/.scout/cache.db`, and the content store
   (`$HOME/.claude/scout/content.db`, or the path `SCOUT_CONTENT_DB` names).

Removing the hooks does not affect the index: `map`, `find`, `refs`, `read`, `impact` and `tests`
answer as before.
