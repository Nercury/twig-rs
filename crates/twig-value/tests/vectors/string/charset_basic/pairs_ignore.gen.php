<?php
// pairs_ignore.json: every pair of the charsets charset_basic converts with //IGNORE and //TRANSLIT//IGNORE (see
// pairs.inc.php).
require __DIR__ . '/pairs.inc.php';
emit('pairs_ignore.gen.php', pair_cases(['//IGNORE', '//TRANSLIT//IGNORE']));
