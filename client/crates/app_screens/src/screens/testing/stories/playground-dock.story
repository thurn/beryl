story table 1600x900 desktop
// The playground dock open over the first of Counterspell's three examples. The testing screen
// finds the card, lists its examples and plays the first, from the session's recorded answers;
// a press on the dock at the table's top edge then opens its list of examples, with the one
// played now selected and "Back" last.
card "Island" battlefield.yours.lands
card "Island" battlefield.yours.lands
card "Counterspell" hand.yours
card library.yours x8
card library.theirs x8
message {"kind":"event","event":"beryl-client-recordings","payload":["a recording"]}
message {"kind":"event","event":"beryl-client-open","payload":"testing"}
message {"kind":"response","command":"playground_find","value":"Counterspell"}
message {"kind":"response","command":"playground_examples","value":{"name":"Counterspell","slug":"counterspell","examples":[{"title":"Play from hand","label":"Default","name":"playground-hand"},{"title":"Play from hand","label":"Counter a creature spell","name":"playground-counter-a-creature-spell-0"},{"title":"Triggers & effects","label":"Counter an instant","name":"playground-counter-an-instant-0"}],"missing":[]}}
message {"kind":"response","command":"playground_start","value":1}
action type field "card" "counterspell"
action press button "Counterspell"
action press button "Play from hand · Default"
action press button "Counterspell · Play from hand · Default (1/3)"
action still
