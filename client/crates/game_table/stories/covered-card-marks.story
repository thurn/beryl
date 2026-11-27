story table 1728x1117 desktop
// Marks of a covered card (P/T plaque, counters, summoning sickness) must stay behind a covering
// card: Dandân behind Lonely Sandbar on the stack while resolving, Control Magic tucked under
// Dandân on the battlefield, and overlapping creatures in a crowded battlefield row.
card "Island" #10 battlefield.yours.lands x3
card "Dandân" #13 battlefield.yours.creatures stats(4/1)
card "Control Magic" #14 battlefield.yours.others
card "Dandân {n}" #20 battlefield.yours.creatures stats(4/1) x24
card "Dandân" #1 stack stats(4/1)
spell {"kind":"spell","paid":[],"slot":1,"targets":[],"x":0}
card "Lonely Sandbar" #2 graveyard.yours
ability 1 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Lonely Sandbar","nth":0,"source":2,"targets":[],"triggered":false,"x":0}

script
{
  "beats": [
    [
      {"act":{"verb":"set","fact":{"kind":"object","object":{"attachedTo":13,"combat":null,"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Control Magic","owner":"you","pt":null,"slot":14,"summoningSick":false,"tapped":false,"token":false,"types":["enchantment"],"zone":"battlefield"}}}},
      {"act":{"verb":"set","fact":{"kind":"object","object":{"attachedTo":null,"combat":null,"controller":"you","counters":[{"count":2,"kind":"plusOne"}],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Dandân","owner":"opponent","pt":{"power":4,"toughness":1},"slot":13,"summoningSick":true,"tapped":false,"token":false,"types":["creature"],"zone":"battlefield"}}}},
      {"act":{"verb":"dwell","subject":{"kind":"ability","id":1}}}
    ],
    [
      {"act":{"verb":"fly","slot":1,"from":"stack","to":"battlefield","controller":"you","owner":"you","why":"resolve","object":{"attachedTo":null,"combat":null,"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Dandân","owner":"you","pt":{"power":4,"toughness":1},"slot":1,"summoningSick":true,"tapped":false,"token":false,"types":["creature"],"zone":"battlefield"}}}
    ]
  ]
}
end-script
