-- breaking: ticket IDs are stored as the hub now stores them, a `T` and then capitals, digits,
-- dots and dashes (`q44s.7` becomes `TQ44S.7`). A tree started from a summary before the hub
-- filed a ticket for it holds a name such as `adhoc20261118055450891`, and a landing tree holds
-- its holder, `<machine>:<handle>`; both are left alone, as is a value already in the new form.
UPDATE trees SET ticket = 'T' || upper(ticket)
  WHERE ticket GLOB '[a-z0-9]*' AND ticket NOT GLOB 'adhoc[0-9]*'
    AND instr(ticket, ':') = 0;
UPDATE grants SET ticket = 'T' || upper(ticket)
  WHERE ticket GLOB '[a-z0-9]*' AND ticket NOT GLOB 'adhoc[0-9]*'
    AND instr(ticket, ':') = 0;
UPDATE pool SET ticket = 'T' || upper(ticket)
  WHERE ticket GLOB '[a-z0-9]*' AND ticket NOT GLOB 'adhoc[0-9]*'
    AND instr(ticket, ':') = 0;
UPDATE shown SET ticket = 'T' || upper(ticket)
  WHERE ticket GLOB '[a-z0-9]*' AND ticket NOT GLOB 'adhoc[0-9]*'
    AND instr(ticket, ':') = 0;
UPDATE claim_activity SET ticket = 'T' || upper(ticket)
  WHERE ticket GLOB '[a-z0-9]*' AND ticket NOT GLOB 'adhoc[0-9]*'
    AND instr(ticket, ':') = 0;
