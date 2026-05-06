/+  default-agent, dbug
|%
+$  versioned-state
  $%  [%0 ~]
  ==
+$  card  card:agent:gall
--
%-  agent:dbug
=|  state=versioned-state
^-  agent:gall
|_  =bowl:gall
+*  this  .
    def   ~(. (default-agent this %|) bowl)
++  on-init   `this
++  on-save   !>(state)
++  on-load
  |=  old=vase
  `this(state !<(versioned-state old))
++  on-poke
  |=  [=mark =vase]
  ^-  (quip card _this)
  :_  this
  [%pass /exit %agent [our.bowl %hood] %poke %drum-exit !>(~)]~
++  on-peek   on-peek:def
++  on-agent  on-agent:def
++  on-arvo   on-arvo:def
++  on-fail   on-fail:def
++  on-watch  on-watch:def
++  on-leave  on-leave:def
--