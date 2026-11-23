story table 1728x1117 desktop
// The opponent activates Currency Converter with Damn tucked under it.
// Damn leaves for the graveyard when the ability resolves.
card "Island" #289 battlefield.theirs.lands
card "Currency Converter" #290 battlefield.theirs.others
card "Island" #295 battlefield.theirs.lands

script
{
  "beats": [
    [
      {"act":{"verb":"appear","from":"nowhere","why":"effect","object":{"attachedTo":null,"combat":null,"controller":"opponent","counters":[],"damage":0,"exiledBy":null,"exiledWith":290,"facing":"up","keywords":[],"name":"Damn","owner":"opponent","pt":null,"slot":292,"summoningSick":false,"tapped":false,"token":false,"types":[],"zone":"exile"}}}
    ]
  ]
}
end-script

script
{
  "beats": [
    [
      {"act":{"id":3,"item":{"ability":2,"controller":"opponent","granted":false,"kind":"ability","name":"Currency Converter","nth":1,"source":290,"targets":[],"triggered":false,"x":0},"verb":"emerge"},"cause":{"kind":"player","player":"opponent"}},
      {"act":{"arrow":"source","from":{"id":3,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":290},"verb":"point"}}
    ],
    [
      {"act":{"object":{"attachedTo":null,"combat":null,"controller":"opponent","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Currency Converter","owner":"opponent","pt":null,"slot":290,"summoningSick":false,"tapped":true,"token":false,"types":["artifact"],"zone":"battlefield"},"verb":"tap"},"cause":{"kind":"player","player":"opponent"}}
    ]
  ]
}
end-script

script
{
  "beats": [
    [
      {"act":{"subject":{"id":3,"kind":"ability"},"verb":"dwell"},"cause":{"id":3,"kind":"ability"}}
    ]
  ],
  "timing": [{"after":[],"ahead":false,"alone":false,"case":"read ability","curve":"linear","delay":0.0,"duration":420.0,"effects":[],"leads":false,"opens":false,"passes":false,"phases":[],"room":{"kind":"step"},"stagger":0.0,"with":false}]
}
end-script

script
{
  "beats": [
    [
      {"act":{"verb":"fly","from":"exile","why":"effect","object":{"attachedTo":null,"combat":null,"controller":"opponent","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Damn","owner":"opponent","pt":null,"slot":292,"summoningSick":false,"tapped":false,"token":false,"types":[],"zone":"graveyard"}},"cause":{"id":3,"kind":"ability"}}
    ],
    [
      {"act":{"verb":"appear","from":"nowhere","why":"created","object":{"attachedTo":null,"combat":null,"controller":"opponent","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Rogue Token","owner":"opponent","pt":{"power":2,"toughness":2},"slot":296,"summoningSick":true,"tapped":false,"token":true,"types":["creature"],"zone":"battlefield"}},"cause":{"id":3,"kind":"ability"}}
    ],
    [
      {"act":{"arrow":"source","from":{"id":3,"kind":"ability"},"span":"end","to":{"kind":"object","slot":290},"verb":"point"}}
    ],
    [
      {"act":{"verb":"vanish","from":"stack","why":"resolve","subject":{"id":3,"kind":"ability"}},"cause":{"id":3,"kind":"ability"}}
    ]
  ]
}
end-script
