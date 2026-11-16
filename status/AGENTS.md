# The bead status site

This directory is a standalone Python project, `beryl_status`, that shows the
beads as a website: a home page of cards in two parts, Active (every bead in
progress) and All (the open beads in five sections: epics, beads people filed,
beads the tools filed by themselves in groups by label, bug reports, and
deferred beads), beside a table of contents that follows the reader's scrolling,
and a page per bead at `beads/<id>` with its status, priority, assignee, epic,
labels, description, blockers, the beads it blocks, subtasks, comments and
changelog. It is read only. `just status` serves it on http://127.0.0.1:8766,
and `just status publish` pushes it to GitHub Pages. The project also holds
**cirrus**, the design system the site is built from, in `src/cirrus/`.

## Priorities

Every change here serves two priorities, in this order.

1. **UI performance.** The site must feel instant, both measured and perceived.
   - Optimize by caching at every layer. The site is static HTML built ahead of
     time. A rebuild renders and writes only pages whose inputs changed
     (`build.PageCache`, keyed by a fingerprint of everything a page shows). The
     server answers with ETags and 304s, and content-hashed assets are cached
     for a year. The service worker (`sw.js`) answers every page from its cache
     at once and revalidates in the background. `cirrus.js` prefetches a link on
     hover, focus or touch, and cards as they scroll into view.
   - Show cached content while new content is fetched, never a blank page. The
     server starts by serving the files the last build left. An open page polls
     `version.json` and swaps in what changed without moving the reader.
     Anything slow (a changelog that `bd` has to be asked for) loads after the
     page, behind a skeleton shaped like what will replace it.
   - Nothing on a page's path waits on `bd`. `bd export` takes about two seconds
     and `bd history --events` half a second per bead, and either can wait on
     Dolt's lock for ten seconds or more. Both run in background threads or
     processes, below normal priority.
   - Measure before and after. A full cold build of about 4,600 beads takes 3 to
     6 seconds; a warm rebuild after a change takes about half a second.
     Navigating to a prefetched page swaps the content in a few milliseconds.
     Keep it that way.
2. **A beautiful design system**, cirrus, described below. Every page is built
   from its components, and a new need becomes a new component in `cirrus`, not
   one-off markup in `beryl_status`.

## Cirrus design system principles

1. **Ruthless simplicity, without sacrificing capability.** Show only what
   matters right now. Every visible element earns its place by helping the user
   understand, decide or act. Remove unnecessary borders, labels, icons,
   controls and decoration, but never at the expense of clarity or
   discoverability. Minimize visible complexity, not functional depth.
2. **Progressive disclosure over permanent clutter.** Reveal complexity in
   context through hovercards, tooltips, popovers, expandable sections and
   contextual actions. Keep primary workflows immediately accessible, and put
   secondary information and advanced controls one interaction away. Hover
   interactions also work through click, touch and keyboard focus. Make the
   common path effortless and the advanced path discoverable.
3. **Whitespace and typography are the interface.** Use generous, deliberate
   whitespace to show relationships, separate concepts and guide attention. Let
   typography carry hierarchy through size, weight, contrast and spacing rather
   than boxes and dividers. Use the one spacing scale and the restrained type
   scale in `css/1-tokens.css`. Structure through space; communicate through
   type.
4. **Visual hierarchy should feel inevitable.** Every screen has an unmistakable
   focal point, clear primary actions and a natural reading order. Use contrast,
   alignment, proximity and scale to guide the eye without demanding attention.
   Secondary elements recede; important information stays prominent. The user's
   eye should know where to go before they consciously decide.
5. **Aesthetic beauty comes from precision and restraint.** Create elegance
   through attention to detail: harmonious proportions, consistent alignment,
   refined typography, subtle colour, carefully tuned spacing and purposeful
   motion. Prefer quiet sophistication over visual novelty. Make every detail
   feel intentional, and nothing feel excessive.
6. **Motion should communicate, not decorate.** Use fluid, purposeful motion to
   make the interface feel responsive, coherent and alive. Transitions preserve
   spatial continuity: where elements come from, where they go and how states
   relate. Loading states communicate progress, reduce perceived waiting and
   keep the layout stable, through skeletons, subtle indicators and graceful
   reveals. Favour natural easing, restrained durations and smooth
   microinteractions. Never let animation delay interaction, and honour
   `prefers-reduced-motion`.

## Cirrus

- `components.py`: functions that return HTML strings: `page` (the document,
  with the header and `<main>` that navigation swaps), `title`, `part` (a
  top-level division holding sections), `section`, `toc` and `with_toc` (a table
  of contents, a sticky rail on a wide screen and a sticky row of links on a
  narrow one), `grid` and `card`, `rows` and `row`, `layout` and `meta` (a
  reading column with a property rail), `chip`, `dot`, `progress`, `tip` (a
  tooltip), `link` with a `template` (a hovercard), `disclosure`, `timeline`,
  `comment`, `time` (a relative time), `skeleton`, `include` (a fragment loaded
  after the page), `empty` and `prose`. Every text passes through `esc`.
- `markdown.py`: a safe subset of Markdown that escapes everything it does not
  write itself, with an `autolink` hook (the site links bead ids with it).
- `assets.py`: writes the static files under content-hashed names. `static/css/`
  holds the stylesheet in order: tokens (colours for light and dark, the spacing
  and type scales, easing and durations), layout, components, motion. They are
  joined into one `cirrus.css`.
- `static/cirrus.js`: navigation, prefetching, popovers, fragments, relative
  times, freshness and the table of contents' mark. Every page works without it.
  `static/sw.js` is the service worker.
- Colour is for meaning only: the tones (`neutral`, `accent`, `red`, `orange`,
  `amber`, `green`, `blue`, `violet`, `muted`) mark status and priority. Use a
  token, never a literal colour, outside `1-tokens.css`.
- System fonts only (SF on Apple devices), so no page waits on a font.

## Layout of `beryl_status`

- `beads.py`: reads `bd export` into `Bead`s and indexes them (`Index`: parent,
  children, blockers, the beads each one blocks and related beads). `Index.home`
  sorts the open beads into the home page's sections (`Home`). `AUTOMATIC` lists
  the groups of beads the tools file by themselves, by label: add a group there
  when a tool starts filing a new kind of bead.
- `changelog.py`: each bead's audit events from `bd history <id> --events`,
  cached as JSON under `$BERYL_CACHE/status/changelog/` and fetched again only
  when the bead's `updated_at` moves. A bead with no recorded events shows the
  lifecycle its timestamps record.
- `render.py`: the pages. Links are relative (`beads/by-x` from the home page,
  `by-x` between beads), so the same files work at a server's root and under a
  GitHub Pages project folder. Only the 404 page, which a server answers at any
  address, uses an absolute `<base>`.
- `build.py`: writes the site into `$BERYL_CACHE/status/sites/<code version>/`,
  a file only when its bytes changed, so unchanged pages keep their ETags and
  browser caches. Each version of this project's code (`paths.code_version`, a
  digest of its source) builds into its own folder, so a worktree's server and
  the agent's publish from the main checkout never overwrite each other's pages.
  A server starting with new code answers from the newest other folder
  (`paths.last_site`, in place, since copying it could take 30 seconds on a busy
  disk) until its own first build finishes. Folders unused for two days are
  deleted.
- `site.py`: the shared state. `refresh` exports only when the database's files
  changed, and `fill` fetches changelogs that are not current, open beads first.
- `serve.py`: the server (GET and HEAD only). It fetches a changelog fragment
  from `bd` when a page asks for one that is missing or out of date. It reads
  its own code every two seconds and, once a change has settled and imports
  (`CodeWatch`), restarts itself on the new code, so a server in the main
  checkout follows `staging`. An open page whose assets changed reloads in
  place.
- `publish.py`: `just status publish` commits the site with its own index file
  (never a checkout) as one parentless commit and force-pushes it to `gh-pages`,
  so the branch never grows a history. It turns GitHub Pages on the first time.
  The beads are already public in `refs/dolt/data`.
- `agent.py`: `just status install` loads a launchd agent that publishes when
  the beads database's files change, no sooner than ten minutes after its last
  run, because GitHub Pages builds a branch at most about ten times an hour, and
  hourly in case launchd missed a change. `just status uninstall` removes it.

## Rules

- Standard library only, in Python and in the browser: no frameworks, no build
  step and no CDN.
- Nothing here imports from `tools/`, and nothing outside imports this package.
  Where a rule must match the tools (the cache's location), copy it and say
  where it came from.
- The repository is public. Tests use the small synthetic export in
  `tests/conftest.py`. Never commit a real export or changelog.
- The `status` check lane (`tools/src/beryl_tools/standalone.py`) runs black,
  pyre (strict) and pytest here, and only a change under `status/` selects it.
  To run its steps by hand from this directory:
  `uv run --project ../tools python -m pytest`, and the same with
  `python -m black --check .` and `pyre --noninteractive check`.
- To look at a change, run `just status --port <free port>` and open it in a
  browser. Check a phone width (375 px) and both colour schemes. The
  `status-dashboard` skill sums up what every change here is held to.
