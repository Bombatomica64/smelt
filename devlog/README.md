# Smelt dev log

A once-a-day, honest dev-log routine for posting about Smelt on X (Twitter).
It is deliberately **not** a promo machine: it puts gripes next to wins and is
fine saying that a lot of the code is AI-generated.

Nothing here posts to X automatically. Every run produces a **draft you read
and post by hand**, so you stay in control of what ships.

## How it works

The `/devlog` skill (`skills/devlog/SKILL.md`) does the writing. Each run it:

1. Looks at git history since the last log (`state.json` tracks the cutoff).
2. Scans `blocker-logs/`, `Test-TODO.md`, and `IMPLEMENTATION_CHECKLIST.md` for
   real, current gripes.
3. Folds in whatever you wrote in [`NOTES.md`](./NOTES.md) — the highest-signal
   input.
4. Drafts a single ≤280-char post (or offers a short thread), writes it to
   `posts/YYYY-MM-DD.md`, and shows it to you to copy-paste.
5. Advances `state.json` so tomorrow's window starts where today's ended.

## Running it

**Manually:** open a session in this repo and run `/devlog`. Read the draft,
tweak if you want, paste it to X.

**Once a day (scheduled web session):** set up a scheduled session in Claude
Code on the web (https://claude.ai/code → the repo's environment → schedule a
recurring session). Use this as the session prompt:

```
Run the /devlog routine for Smelt. Draft today's dev-log post following
skills/devlog/SKILL.md, show me the draft, and commit the draft + state update
to the devlog branch. Do not post to X.
```

Set it to run once a day at whatever time you like. When it fires, the draft
will be waiting in `devlog/posts/` and in the session transcript for you to
post.

> Why a session and not a cron/bash script: a templated script writes robotic
> tweets. Letting the model read the actual project state each day is what makes
> the log read like a person wrote it — gripes and all.

## Files

- `NOTES.md` — your freeform inbox for the next post. Write here between runs.
- `posts/` — dated drafts. History of what the log said.
- `state.json` — `last_commit` / `last_run`; the cutoff for "what changed".

## Notes on X limits

Drafts target the free-tier 280-character limit.

The repo link never goes in the main post — X demotes posts with external links,
and a link costs ~23 chars. When a post is worth linking, the draft includes a
`reply:` line: post the main tweet, then reply to it with that link. Pure-gripe
days skip the link entirely.
