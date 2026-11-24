story table 1600x900 desktop
neutral

// Creature and land alternate. Control changes every two cards and every third card is tapped.
first-slot 1
repeat 4
  card "Stress {id:03}" battlefield.yours.creatures stats(2/2) tapped
  card "Stress {id:03}" battlefield.yours.lands
  card "Stress {id:03}" battlefield.theirs.creatures stats(2/2)
  card "Stress {id:03}" battlefield.theirs.lands tapped
  card "Stress {id:03}" battlefield.yours.creatures stats(2/2)
  card "Stress {id:03}" battlefield.yours.lands
  card "Stress {id:03}" battlefield.theirs.creatures stats(2/2) tapped
  card "Stress {id:03}" battlefield.theirs.lands
  card "Stress {id:03}" battlefield.yours.creatures stats(2/2)
  card "Stress {id:03}" battlefield.yours.lands tapped
  card "Stress {id:03}" battlefield.theirs.creatures stats(2/2)
  card "Stress {id:03}" battlefield.theirs.lands
end-repeat
card "Stress {id:03}" #49 battlefield.yours.creatures stats(2/2) tapped
card "Stress {id:03}" #50 battlefield.yours.lands

// Each spell and each ability targets one of the fifty permanents.
card "Stress {id:03}" #51 stack
spell {"kind":"spell","paid":[],"slot":51,"targets":[{"kind":"object","slot":1}],"x":0}
ability 1 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 001","nth":0,"source":1,"targets":[{"kind":"object","slot":2}],"triggered":true,"x":0}
card "Stress {id:03}" #52 stack
spell {"kind":"spell","paid":[],"slot":52,"targets":[{"kind":"object","slot":3}],"x":0}
ability 2 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 003","nth":0,"source":3,"targets":[{"kind":"object","slot":4}],"triggered":true,"x":0}
card "Stress {id:03}" #53 stack
spell {"kind":"spell","paid":[],"slot":53,"targets":[{"kind":"object","slot":5}],"x":0}
ability 3 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 005","nth":0,"source":5,"targets":[{"kind":"object","slot":6}],"triggered":true,"x":0}
card "Stress {id:03}" #54 stack
spell {"kind":"spell","paid":[],"slot":54,"targets":[{"kind":"object","slot":7}],"x":0}
ability 4 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 007","nth":0,"source":7,"targets":[{"kind":"object","slot":8}],"triggered":true,"x":0}
card "Stress {id:03}" #55 stack
spell {"kind":"spell","paid":[],"slot":55,"targets":[{"kind":"object","slot":9}],"x":0}
ability 5 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 009","nth":0,"source":9,"targets":[{"kind":"object","slot":10}],"triggered":true,"x":0}
card "Stress {id:03}" #56 stack
spell {"kind":"spell","paid":[],"slot":56,"targets":[{"kind":"object","slot":11}],"x":0}
ability 6 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 011","nth":0,"source":11,"targets":[{"kind":"object","slot":12}],"triggered":true,"x":0}
card "Stress {id:03}" #57 stack
spell {"kind":"spell","paid":[],"slot":57,"targets":[{"kind":"object","slot":13}],"x":0}
ability 7 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 013","nth":0,"source":13,"targets":[{"kind":"object","slot":14}],"triggered":true,"x":0}
card "Stress {id:03}" #58 stack
spell {"kind":"spell","paid":[],"slot":58,"targets":[{"kind":"object","slot":15}],"x":0}
ability 8 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 015","nth":0,"source":15,"targets":[{"kind":"object","slot":16}],"triggered":true,"x":0}
card "Stress {id:03}" #59 stack
spell {"kind":"spell","paid":[],"slot":59,"targets":[{"kind":"object","slot":17}],"x":0}
ability 9 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 017","nth":0,"source":17,"targets":[{"kind":"object","slot":18}],"triggered":true,"x":0}
card "Stress {id:03}" #60 stack
spell {"kind":"spell","paid":[],"slot":60,"targets":[{"kind":"object","slot":19}],"x":0}
ability 10 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 019","nth":0,"source":19,"targets":[{"kind":"object","slot":20}],"triggered":true,"x":0}
card "Stress {id:03}" #61 stack
spell {"kind":"spell","paid":[],"slot":61,"targets":[{"kind":"object","slot":21}],"x":0}
ability 11 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 021","nth":0,"source":21,"targets":[{"kind":"object","slot":22}],"triggered":true,"x":0}
card "Stress {id:03}" #62 stack
spell {"kind":"spell","paid":[],"slot":62,"targets":[{"kind":"object","slot":23}],"x":0}
ability 12 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 023","nth":0,"source":23,"targets":[{"kind":"object","slot":24}],"triggered":true,"x":0}
card "Stress {id:03}" #63 stack
spell {"kind":"spell","paid":[],"slot":63,"targets":[{"kind":"object","slot":25}],"x":0}
ability 13 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 025","nth":0,"source":25,"targets":[{"kind":"object","slot":26}],"triggered":true,"x":0}
card "Stress {id:03}" #64 stack
spell {"kind":"spell","paid":[],"slot":64,"targets":[{"kind":"object","slot":27}],"x":0}
ability 14 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 027","nth":0,"source":27,"targets":[{"kind":"object","slot":28}],"triggered":true,"x":0}
card "Stress {id:03}" #65 stack
spell {"kind":"spell","paid":[],"slot":65,"targets":[{"kind":"object","slot":29}],"x":0}
ability 15 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 029","nth":0,"source":29,"targets":[{"kind":"object","slot":30}],"triggered":true,"x":0}
card "Stress {id:03}" #66 stack
spell {"kind":"spell","paid":[],"slot":66,"targets":[{"kind":"object","slot":31}],"x":0}
ability 16 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 031","nth":0,"source":31,"targets":[{"kind":"object","slot":32}],"triggered":true,"x":0}
card "Stress {id:03}" #67 stack
spell {"kind":"spell","paid":[],"slot":67,"targets":[{"kind":"object","slot":33}],"x":0}
ability 17 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 033","nth":0,"source":33,"targets":[{"kind":"object","slot":34}],"triggered":true,"x":0}
card "Stress {id:03}" #68 stack
spell {"kind":"spell","paid":[],"slot":68,"targets":[{"kind":"object","slot":35}],"x":0}
ability 18 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 035","nth":0,"source":35,"targets":[{"kind":"object","slot":36}],"triggered":true,"x":0}
card "Stress {id:03}" #69 stack
spell {"kind":"spell","paid":[],"slot":69,"targets":[{"kind":"object","slot":37}],"x":0}
ability 19 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 037","nth":0,"source":37,"targets":[{"kind":"object","slot":38}],"triggered":true,"x":0}
card "Stress {id:03}" #70 stack
spell {"kind":"spell","paid":[],"slot":70,"targets":[{"kind":"object","slot":39}],"x":0}
ability 20 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 039","nth":0,"source":39,"targets":[{"kind":"object","slot":40}],"triggered":true,"x":0}
card "Stress {id:03}" #71 stack
spell {"kind":"spell","paid":[],"slot":71,"targets":[{"kind":"object","slot":41}],"x":0}
ability 21 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 041","nth":0,"source":41,"targets":[{"kind":"object","slot":42}],"triggered":true,"x":0}
card "Stress {id:03}" #72 stack
spell {"kind":"spell","paid":[],"slot":72,"targets":[{"kind":"object","slot":43}],"x":0}
ability 22 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 043","nth":0,"source":43,"targets":[{"kind":"object","slot":44}],"triggered":true,"x":0}
card "Stress {id:03}" #73 stack
spell {"kind":"spell","paid":[],"slot":73,"targets":[{"kind":"object","slot":45}],"x":0}
ability 23 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 045","nth":0,"source":45,"targets":[{"kind":"object","slot":46}],"triggered":true,"x":0}
card "Stress {id:03}" #74 stack
spell {"kind":"spell","paid":[],"slot":74,"targets":[{"kind":"object","slot":47}],"x":0}
ability 24 {"ability":0,"controller":"opponent","granted":false,"kind":"ability","name":"Stress 047","nth":0,"source":47,"targets":[{"kind":"object","slot":48}],"triggered":true,"x":0}
card "Stress {id:03}" #75 stack
spell {"kind":"spell","paid":[],"slot":75,"targets":[{"kind":"object","slot":49}],"x":0}
ability 25 {"ability":0,"controller":"you","granted":false,"kind":"ability","name":"Stress 049","nth":0,"source":49,"targets":[{"kind":"object","slot":50}],"triggered":true,"x":0}

// Open all fifty arrows together.
script
{
  "beats": [
    [
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":51},"span":"begin","to":{"kind":"object","slot":1},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":1,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":2},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":52},"span":"begin","to":{"kind":"object","slot":3},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":2,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":4},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":53},"span":"begin","to":{"kind":"object","slot":5},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":3,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":6},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":54},"span":"begin","to":{"kind":"object","slot":7},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":4,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":8},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":55},"span":"begin","to":{"kind":"object","slot":9},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":5,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":10},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":56},"span":"begin","to":{"kind":"object","slot":11},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":6,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":12},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":57},"span":"begin","to":{"kind":"object","slot":13},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":7,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":14},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":58},"span":"begin","to":{"kind":"object","slot":15},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":8,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":16},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":59},"span":"begin","to":{"kind":"object","slot":17},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":9,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":18},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":60},"span":"begin","to":{"kind":"object","slot":19},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":10,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":20},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":61},"span":"begin","to":{"kind":"object","slot":21},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":11,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":22},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":62},"span":"begin","to":{"kind":"object","slot":23},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":12,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":24},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":63},"span":"begin","to":{"kind":"object","slot":25},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":13,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":26},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":64},"span":"begin","to":{"kind":"object","slot":27},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":14,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":28},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":65},"span":"begin","to":{"kind":"object","slot":29},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":15,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":30},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":66},"span":"begin","to":{"kind":"object","slot":31},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":16,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":32},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":67},"span":"begin","to":{"kind":"object","slot":33},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":17,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":34},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":68},"span":"begin","to":{"kind":"object","slot":35},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":18,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":36},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":69},"span":"begin","to":{"kind":"object","slot":37},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":19,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":38},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":70},"span":"begin","to":{"kind":"object","slot":39},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":20,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":40},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":71},"span":"begin","to":{"kind":"object","slot":41},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":21,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":42},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":72},"span":"begin","to":{"kind":"object","slot":43},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":22,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":44},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":73},"span":"begin","to":{"kind":"object","slot":45},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":23,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":46},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":74},"span":"begin","to":{"kind":"object","slot":47},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":24,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":48},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"kind":"object","slot":75},"span":"begin","to":{"kind":"object","slot":49},"verb":"point"}},
      {"act":{"arrow":"friendly-target","from":{"id":25,"kind":"ability"},"span":"begin","to":{"kind":"object","slot":50},"verb":"point"}}
    ]
  ]
}
end-script

// Close the top ability's arrow as it resolves.
script
{
  "beats": [
    [
      {"act":{"arrow":"friendly-target","from":{"id":25,"kind":"ability"},"span":"end","to":{"kind":"object","slot":50},"verb":"point"}},
      {"act":{"from":"stack","subject":{"id":25,"kind":"ability"},"verb":"vanish","why":"resolve"}}
    ]
  ]
}
end-script
