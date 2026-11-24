story table 1728x1117 desktop
card "Beza, the Bounding Spring" #20 battlefield.yours.creatures stats(4/5)
card "Fish Token" #512 battlefield.yours.creatures stats(1/1) token x2
card "Spirit Token" #514 battlefield.yours.creatures stats(1/1) token
card "Snapcaster Mage" #294 battlefield.theirs.creatures stats(2/1)
card "Jace, Vryn's Prodigy" #298 battlefield.theirs.creatures stats(0/2) tapped
card "Jace, Wielder of Mysteries" #293 battlefield.theirs.others

// Beza attacks Jace.
script
{
  "beats": [
    [
      {"act":{"object":{"attachedTo":null,"combat":{"blocked":false,"kind":"attacking","planeswalker":293},"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Beza, the Bounding Spring","owner":"you","pt":{"power":4,"toughness":5},"slot":20,"summoningSick":false,"tapped":true,"token":false,"types":["creature"],"zone":"battlefield"},"verb":"tap"}},
      {"act":{"arrow":"attack","from":{"kind":"object","slot":20},"span":"begin","to":{"kind":"object","slot":293},"verb":"point"}}
    ]
  ]
}
end-script

// Snapcaster blocks Beza across the narrow gap between the creature rows.
script
{
  "beats": [
    [
      {"act":{"fact":{"kind":"object","object":{"attachedTo":null,"combat":{"attacker":20,"kind":"blocking"},"controller":"opponent","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Snapcaster Mage","owner":"opponent","pt":{"power":2,"toughness":1},"slot":294,"summoningSick":false,"tapped":false,"token":false,"types":["creature"],"zone":"battlefield"}},"verb":"set"}},
      {"act":{"arrow":"block","from":{"kind":"object","slot":294},"span":"begin","to":{"kind":"object","slot":20},"verb":"point"}},
      {"act":{"fact":{"kind":"object","object":{"attachedTo":null,"combat":{"blocked":true,"kind":"attacking","planeswalker":293},"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Beza, the Bounding Spring","owner":"you","pt":{"power":4,"toughness":5},"slot":20,"summoningSick":false,"tapped":true,"token":false,"types":["creature"],"zone":"battlefield"}},"verb":"set"}},
      {"act":{"arrow":"attack","from":{"kind":"object","slot":20},"span":"end","to":{"kind":"object","slot":293},"verb":"point"}}
    ]
  ]
}
end-script

// The combat records and the last connector clear.
script
{
  "beats": [
    [
      {"act":{"verb":"set","fact":{"kind":"object","object":{"attachedTo":null,"combat":null,"controller":"opponent","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Snapcaster Mage","owner":"opponent","pt":{"power":2,"toughness":1},"slot":294,"summoningSick":false,"tapped":false,"token":false,"types":["creature"],"zone":"battlefield"}}}},
      {"act":{"verb":"set","fact":{"kind":"object","object":{"attachedTo":null,"combat":null,"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Beza, the Bounding Spring","owner":"you","pt":{"power":4,"toughness":5},"slot":20,"summoningSick":false,"tapped":true,"token":false,"types":["creature"],"zone":"battlefield"}}}},
      {"act":{"arrow":"block","from":{"kind":"object","slot":294},"span":"end","to":{"kind":"object","slot":20},"verb":"point"}}
    ]
  ]
}
end-script
