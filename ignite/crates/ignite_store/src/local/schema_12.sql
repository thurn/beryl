-- Schema version 12. The verb process that runs a handle's verb, if one does (see
-- `ignite_daemon`'s `verbproc`): its process ID, the Ignite commit of the executable it runs
-- under, and the host protocol version it speaks. The verb process writes them when it starts,
-- so `ignite status` can say which build a verb runs under after an upgrade. A verb the daemon
-- runs on a thread of its own leaves them empty.
--
-- Additive: three new nullable columns.

ALTER TABLE handles ADD COLUMN pid INTEGER;
ALTER TABLE handles ADD COLUMN executable TEXT;
ALTER TABLE handles ADD COLUMN protocol INTEGER;
