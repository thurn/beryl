story table 1728x1117 desktop
card "Beza, the Bounding Spring" #1 battlefield.yours.creatures stats(4/5)
card "Fish Token" #2 battlefield.yours.creatures stats(1/1) token

// The second Fish appears under the first Fish in the same pile.
script
{
  "beats": [[
    {"act":{"verb":"appear","from":"nowhere","why":"created","object":{"attachedTo":null,"combat":null,"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Fish Token","owner":"you","pt":{"power":1,"toughness":1},"slot":3,"summoningSick":false,"tapped":false,"token":true,"types":["creature"],"zone":"battlefield"}}}
  ]]
}
end-script

// The two Fish separate when an effect changes the second token's power and toughness.
script
{
  "beats": [[
    {"act":{"verb":"set","fact":{"kind":"object","object":{"attachedTo":null,"combat":null,"controller":"you","counters":[],"damage":0,"exiledBy":null,"exiledWith":null,"facing":"up","keywords":[],"name":"Fish Token","owner":"you","pt":{"power":2,"toughness":3},"slot":3,"summoningSick":false,"tapped":false,"token":true,"types":["creature"],"zone":"battlefield"}}}}
  ]]
}
end-script
