---
name: status-dashboard
description: Work on the bead status site (`just status`, status/) or its cirrus design system. Holds every change there to measured performance, cirrus's components and principles, and a check in a real browser before it lands. Use when changing anything under status/.
---

# Working on the status dashboard

Read `status/AGENTS.md` first, then hold every change to three things.
**Validate it for real**: add or update a test in `status/tests/`, run the
`status` lane's black, pyre and pytest, then serve it with
`just status --port <free port>` and look at the page you changed in a browser,
at desktop and 375 px widths and in both colour schemes, before you push.
**Measure performance before and after**: time the cold and warm rebuilds and
the render of the page you touched, keep navigation to a prefetched page at a
few milliseconds, never put a call to `bd` on a page's path, and give the
numbers in your commit. **Keep the UI consistent**: build from cirrus's
components and tokens, add a component to `src/cirrus/` rather than one-off
markup or a literal colour, and judge the result against the six cirrus
principles: an element that does not help the reader understand, decide or act
is removed or moved one interaction away.
