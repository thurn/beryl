story element 810x380 desktop
region "Start panel" #1 24,24 190x146 | widget=start-panel
  text "Game options" #2 44,44 150x32
list "Choices" #3 236,24 160x146 | widget=list
  text "First" #4 252,44 128x30
  text "Second" #5 252,84 128x30
text "Tooltip" #6 428,24 160x40 | widget=tooltip text="More information"
list "Menu" #7 24,200 220x130 | widget=menu text="Normal row"
list "Checked menu" #8 282,200 220x130 selected | widget=menu text="Checked row"
list "Disabled menu" #9 540,200 220x130 disabled | widget=menu text="Disabled row"
