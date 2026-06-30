# Multitask agent prompts

Launch **three agents in Multitask mode**, one prompt per agent. Each agent must read `web/DESIGN_BRIEF.md` first.

## Before launching agents

Worktrees must include the latest baseline (including the PEG build fix). From the main repo:

```bash
for wt in ../pest-to-marser-design-welcome ../pest-to-marser-design-playground ../pest-to-marser-design-clarity; do
  git -C "$wt" merge main -m "sync: merge baseline fixes from main"
done
```

Or recreate worktrees: remove the three `pest-to-marser-design-*` directories with `git worktree remove`, delete the `design/*` branches, and run `./web/setup-design-worktrees.sh` again from `main`.

Worktree paths:

| Branch | Worktree |
|--------|----------|
| `design/welcome` | `../pest-to-marser-design-welcome` |
| `design/playground` | `../pest-to-marser-design-playground` |
| `design/clarity` | `../pest-to-marser-design-clarity` |

If not using worktrees, checkout the branch in a separate clone or ensure only one agent edits at a time.

---

## Agent 1 — `design/welcome` (First-run friend)

```
Read web/DESIGN_BRIEF.md in full, then redesign the frontend on branch design/welcome.

Work in the design/welcome worktree if it exists (../pest-to-marser-design-welcome), otherwise checkout that branch.

Direction: "First-run friend" — optimize for someone landing with zero context.

Goals:
- Within 5 seconds they know: paste grammar → get Rust parser, in the browser, free
- One obvious primary action: try an example (make the example control feel like the main CTA)
- Shrink or collapse the intro by default; first visit can show a short welcome, return visits see editors immediately
- Reduce toolbar clutter — group secondary actions (share, open file) without hiding them
- Friendly, encouraging microcopy (not cheesy)

Constraints: follow DESIGN_BRIEF.md DOM contract and feature list. Change index.html, styles.css, and ui.js tour/intro copy only as needed.

When done: run npm run build from web/, fix any issues, commit on design/welcome with message "design(welcome): first-run friendly UI".
```

---

## Agent 2 — `design/playground` (Playground)

```
Read web/DESIGN_BRIEF.md in full, then redesign the frontend on branch design/playground.

Work in the design/playground worktree if it exists (../pest-to-marser-design-playground), otherwise checkout that branch.

Direction: "Playground" — make tinkering feel fun and low-stakes.

Goals:
- Editors dominate the viewport; chrome is minimal
- Visually communicate "change this → watch that update" (labels, subtle animation, or split layout cues)
- Example grammars feel like playable presets (visual hierarchy on the example picker)
- Empty or initial state nudges: "pick an example or start typing" near the grammar editor
- Optional: small delight (hover states, success feedback when conversion succeeds) without hurting performance

Constraints: follow DESIGN_BRIEF.md DOM contract and feature list. Change index.html, styles.css, and ui.js only as needed.

When done: run npm run build from web/, fix any issues, commit on design/playground with message "design(playground): playground-focused UI".
```

---

## Agent 3 — `design/clarity` (Clarity first)

```
Read web/DESIGN_BRIEF.md in full, then redesign the frontend on branch design/clarity.

Work in the design/clarity worktree if it exists (../pest-to-marser-design-clarity), otherwise checkout that branch.

Direction: "Clarity first" — scannable hierarchy and progressive disclosure.

Goals:
- Clear 3-step mental model visible at a glance: (1) choose syntax & example (2) edit grammar (3) copy/download Rust
- Typography and spacing that make skimming easy; section headers that don't shout
- Progressive disclosure: advanced options (trace, comments, limitations) de-emphasized until needed
- Intro content restructured as short bullets or cards, not paragraphs
- Strong focus states and contrast for accessibility

Constraints: follow DESIGN_BRIEF.md DOM contract and feature list. Change index.html, styles.css, and ui.js only as needed.

When done: run npm run build from web/, fix any issues, commit on design/clarity with message "design(clarity): clarity-first UI".
```

---

## After all agents finish

Compare locally:

```bash
cd web
./compare-designs.sh
```

Or open each worktree’s `web/` in a browser on different ports.

Pick a winner, then merge that branch to `main` in a follow-up Agent session.

## Sync worktrees after baseline changes

```bash
for wt in ../pest-to-marser-design-welcome ../pest-to-marser-design-playground ../pest-to-marser-design-clarity; do
  git -C "$wt" merge main -m "sync: merge baseline from main"
done
```
