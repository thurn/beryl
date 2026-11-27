---
name: rush
description: Put this session's Ignite work ahead of the worker pool's until its ticket lands. Only the operator invokes it, by typing /rush.
disable-model-invocation: true
---

# Rush

The operator typed `/rush`, so this session's work must finish before the worker
pool's. Run this before anything else:

```sh
ignite rush
```

From then on every heavy command this session runs (`ignite test`, `check`,
`run`, `land` and the builds they start) is scheduled ahead of every class,
behind only a tip round's build checks and the commands of `incident` and
`justiciar` tickets. It may use every shared core. While one of its commands
holds or waits for cores, the machine starts no new ordinary workers. An
`ignite start` that has to wait for a ticket tree goes ahead of the queue.

- The rush ends by itself when this session's ticket lands, is released or is
  closed. If you have no ticket yet, it attaches to the one you `ignite start`.
- It lapses four hours after you ran `ignite rush`. Run it again to renew it if
  the work will take longer.
- Run `ignite rush end` if the operator says the work no longer needs to come
  first.
- An emergency pause still refuses heavy verbs, and a rush does not change that.
- `ignite rush` refuses when two other sessions already rush on this machine.
  Tell the operator which sessions hold them, and carry on without a rush.

Then carry on with the task the operator gave with `/rush`, or with the task
already in progress in this session.
