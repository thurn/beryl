-- What each machine last sent `POST /next`: the `MachineFree` as JSON and the hub's answer (the
-- reserved ticket, or why nothing was offered), so the dashboard can say why ready tickets wait.
CREATE TABLE machine_asks (
  machine TEXT PRIMARY KEY,
  at TEXT NOT NULL,
  free TEXT NOT NULL,
  answer TEXT NOT NULL
) WITHOUT ROWID;
