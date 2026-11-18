-- What each worker is running, as its daemon reports it with every heartbeat: the `ignite`
-- command, and when it started (or ended, once it has ended).
ALTER TABLE workers ADD COLUMN command TEXT;
ALTER TABLE workers ADD COLUMN command_since TEXT;
ALTER TABLE workers ADD COLUMN command_running INTEGER NOT NULL DEFAULT 0;
