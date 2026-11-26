<?php
// pairs.json: every pair of the charsets charset_basic converts, plain and with //TRANSLIT (see pairs.inc.php).
require __DIR__ . '/pairs.inc.php';
emit('pairs.gen.php', pair_cases(['', '//TRANSLIT']));
