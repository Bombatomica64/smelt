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

This is a personal dev log, not an announcement. The user writes the raw
material; you make it read well. Match these rules:

- **Honest over hype.** Lead with what actually happened, including what broke
  or annoyed you. A good post can be mostly a gripe.
- **No promo language.** Never "excited to announce", "🚀", "game-changer",
  "the future of", call-to-action, or hashtag spam. At most one tag if it's
  genuinely relevant.
- **Slangy and online.** Sound like a dev posting at 1am, not a brand. Lean into
  dev/online slang where it fits — e.g. "ngl", "tbh", "lowkey/highkey", "cooked",
  "this is held together with vibes", "yak-shaving", "skill issue", "it's so
  over / we're so back", "ship it", "jank", "fighting for my life in the borrow
  checker". One or two slang beats per post — don't force it into every line or
  it reads try-hard. Match the user's own phrasing from the interview when they
  give it.
- **AI-generated code is not a secret.** A lot of Smelt's code is AI-generated
  and the user is fine saying so. When it's relevant (a bug an agent caused, a
  refactor an agent did, trusting/distrusting generated output), say it plainly.
  Don't force it into every post.
- **First person, lowercase-friendly, terse.** Dry humor welcome.
- **Concrete.** Name the actual thing — `date-fns` slice, clippy warnings,
  HIR/MIR lowering, a specific commit — not "made great progress."
- **Keep it the user's, not yours.** You're polishing their take, not replacing
  it. Don't invent opinions or details they didn't give.

## Length

- Default to a **single post ≤ 280 characters** (assume free-tier X limits).
- If there's genuinely too much for one post, offer a **2–3 post thread** as an
  alternative, but still give the single-post version first.
- Never pad to fill space. A 120-character post is fine.

## Links

- **Never put the repo link in the main post.** X demotes posts with external
  links, and a link burns ~23 chars of the 280 budget. The main post is content
  only.
- When the post points at something worth clicking, put the repo link in a
  suggested **first reply** instead (added to the draft file under a `reply:`
  line). The user posts the main tweet, then replies to it with the link.
- On a pure-gripe / nothing-to-click day, skip the reply entirely — don't link
  reflexively. Repo: https://github.com/Bombatomica64/smelt

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
   there since the last run is high-signal input — weave it in and then clear the
   consumed lines (move them under a `## Archived` heading, don't delete, so
   there's a record).

5. **Interview the user (interactive runs).** This is the main input — the user
   writes most of the content, you make it good. Use the `AskUserQuestion` tool
   to ask several open questions in one batch so they can answer in their own
   words (they answer via the free-text "Other" field, or just reply in chat).
   Ground the questions in what you found in steps 2–3 so they're not generic.
   Ask roughly these, adapted to the day:
   - **What'd you actually work on today?** (offer the commits you found as a
     jog, but let them rewrite it)
   - **What pissed you off / what's janky right now?** (the gripe — the heart of
     the post)
   - **Anything you're lowkey proud of?**
   - **Anything to say about the AI-written side today?** (optional)
   - **Vibe for today — we so back, or it's so over?**

   Their raw answers are the primary material. If they hand you a full sentence,
   keep their words and just tighten. If they give fragments, you assemble.

   **Unattended / scheduled runs:** if no user is there to answer (e.g. a
   scheduled web session running solo), skip the interview and draft from
   `NOTES.md` + git history instead, then leave the draft for review.

6. **Draft the post.** Shape the interview answers into one honest, slangy post.
   Include at least one real gripe unless the day was genuinely clean. Keep their
   voice; don't add opinions they didn't give. Keep it ≤ 280 chars.

7. **Write the draft to a file**, do not post it:
   - Path: `devlog/posts/YYYY-MM-DD.md` (today's UTC date).
   - Contents: the post text; then, if the post warrants a link, a `reply:` line
     with the suggested first-reply (repo link); then a `---` and a short
     "sources" note listing the commits / blockers / notes / interview answers
     you drew from, and the character count.
   - If the file already exists for today, write `YYYY-MM-DD-2.md` etc.

8. **Show the draft in chat** verbatim, with the character count, and tell the
   user it's ready to copy-paste. If they want changes, iterate — it's their
   voice. Offer the thread version only if step 6 hit the length limit with
   leftover material.

9. **Update state.** Set `last_commit` in `devlog/state.json` to current `HEAD`
   and `last_run` to today's UTC date. This advances the window even if the user
   chooses not to post — a skipped day just means tomorrow's window is larger.

## Hard rules

- Never post to X or any network. Draft only.
- Never invent progress. If the window is empty or boring, say so in the draft
  ("quiet day, mostly CI noise") rather than manufacturing a milestone.
- Keep `git status` clean-ish: the only files this routine writes are under
  `devlog/`.
- Don't regenerate or touch generated Rust files (see AGENTS.md).
