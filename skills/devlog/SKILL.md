---
name: devlog
description: Draft an honest, once-a-day X (Twitter) dev-log post about Smelt. Use when the user wants their daily dev log, runs /devlog, or a scheduled web session fires the daily devlog routine. Reads git history since the last log, scans blockers/TODOs for gripes, folds in the user's freeform notes, and writes a draft post for manual review — it NEVER posts anything itself.
---

# Daily Dev Log (X / Twitter)

Draft one honest dev-log post about Smelt, written the way the maintainer
actually talks about the project — wins next to gripes, no marketing voice.
The output is a **draft for the user to read and post manually**. This routine
must never publish to X, never call an external API, and never pretend a post
was sent.

## Voice

This is a personal dev log, not an announcement. Match these rules:

- **Honest over hype.** Lead with what actually happened, including what broke
  or annoyed you. A good post can be mostly a gripe.
- **No promo language.** Never "excited to announce", "🚀", "game-changer",
  "the future of", call-to-action, or hashtag spam. At most one tag if it's
  genuinely relevant.
- **AI-generated code is not a secret.** A lot of Smelt's code is AI-generated
  and the user is fine saying so. When it's relevant (a bug an agent caused, a
  refactor an agent did, trusting/distrusting generated output), say it plainly.
  Don't force it into every post.
- **First person, lowercase-friendly, terse.** Sound like a tired-but-curious
  engineer, not a brand. Dry humor is welcome.
- **Concrete.** Name the actual thing — `date-fns` slice, clippy warnings,
  HIR/MIR lowering, a specific commit — not "made great progress."

## Length

- Default to a **single post ≤ 280 characters** (assume free-tier X limits).
- If there's genuinely too much for one post, offer a **2–3 post thread** as an
  alternative, but still give the single-post version first.
- Never pad to fill space. A 120-character post is fine.

## Procedure

1. **Find the window.** Read `devlog/state.json` for `last_commit`. The window
   is `git log <last_commit>..HEAD`. If the file is missing or the commit is
   unknown, fall back to the last 24h: `git log --since="1 day ago"`.

2. **Gather wins (git history).**
   - `git log <last_commit>..HEAD --oneline`
   - For anything ambiguous, `git show --stat <sha>` to see what really changed.
   - Ignore pure noise commits (`chore(ci): update coverage report`, golden
     output refreshes) unless that IS the story.

3. **Gather gripes.** Skim for current pain, don't deep-read everything:
   - `blocker-logs/` — newest files (`ls -t blocker-logs | head`); these are the
     active failures and frustrations.
   - `Test-TODO.md` and `IMPLEMENTATION_CHECKLIST.md` — unchecked / open items
     near recently touched areas.
   - Optionally `cargo test`/`cargo clippy` state if a gripe needs a live number,
     but don't run a full suite just to write a tweet — cite what the logs say.

4. **Fold in the user's notes.** Read `devlog/NOTES.md`. Anything the user jotted
   there since the last run is the highest-signal input — weave it in and then
   clear the consumed lines (move them under a `## Archived` heading, don't
   delete, so there's a record).

5. **Draft the post.** One honest paragraph or a few short lines. Include at least
   one real gripe unless the day was genuinely clean. Keep it ≤ 280 chars.

6. **Write the draft to a file**, do not post it:
   - Path: `devlog/posts/YYYY-MM-DD.md` (today's UTC date).
   - Contents: the post text, then a `---` and a short "sources" note listing the
     commits / blockers / notes you drew from, and the character count.
   - If the file already exists for today, write `YYYY-MM-DD-2.md` etc.

7. **Show the draft in chat** verbatim, with the character count, and tell the
   user it's ready to copy-paste. Offer the thread version only if step 5 hit the
   length limit with leftover material.

8. **Update state.** Set `last_commit` in `devlog/state.json` to current `HEAD`
   and `last_run` to today's UTC date. This advances the window even if the user
   chooses not to post — a skipped day just means tomorrow's window is larger.

## Hard rules

- Never post to X or any network. Draft only.
- Never invent progress. If the window is empty or boring, say so in the draft
  ("quiet day, mostly CI noise") rather than manufacturing a milestone.
- Keep `git status` clean-ish: the only files this routine writes are under
  `devlog/`.
- Don't regenerate or touch generated Rust files (see AGENTS.md).
