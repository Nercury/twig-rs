/*!re2c
	re2c:api = custom;
	re2c:define:YYCTYPE = u8;
	re2c:define:YYPEEK = "s.str[cursor]";
	re2c:define:YYSKIP = "cursor += 1;";
	re2c:define:YYBACKUP = "marker = cursor;";
	re2c:define:YYRESTORE = "cursor = marker;";
	re2c:define:YYLESSTHAN = "s.lim - cursor < @@{len}";
	re2c:define:YYFILL = "return EOI;";
	re2c:yyfill:parameter = 0;
	re2c:unsafe = 0;

	/* so that vim highlights correctly */
	recurrences
	{
		init!(s, cursor, str, ptr);
		ptr += 1;
		// `int` = `timelib_ull`: truncation.
		s.recurrences = get_unsigned_nr(&str, &mut ptr, 9) as i32;
		s.have_recurrences = true;
		return TIMELIB_PERIOD;
	}

	datetimebasic| datetimeextended
	{
		let current_is_end = if s.have_date || s.have_period {
			s.have_end_date = true;
			true
		} else {
			s.have_begin_date = true;
			false
		};
		init!(s, cursor, str, ptr);
		let y = get_nr(&str, &mut ptr, 4);
		let m = get_nr(&str, &mut ptr, 2);
		let d = get_nr(&str, &mut ptr, 2);
		let h = get_nr(&str, &mut ptr, 2);
		let i = get_nr(&str, &mut ptr, 2);
		let sec = get_nr(&str, &mut ptr, 2);
		let current = if current_is_end { &mut s.end } else { &mut s.begin };
		current.y = y;
		current.m = m;
		current.d = d;
		current.h = h;
		current.i = i;
		current.s = sec;
		s.have_date = true;
		return TIMELIB_ISO_DATE;
	}

	period
	{
		let mut in_time = false;
		init!(s, cursor, str, ptr);
		ptr += 1;
		loop {
			if at(&str, ptr) == b'T' {
				in_time = true;
				ptr += 1;
			}
			if at(&str, ptr) == 0 {
				s.add_error("Missing expected time part");
				break;
			}

			// `timelib_sll` = `timelib_ull`: two's complement reinterpretation.
			let nr = get_unsigned_nr(&str, &mut ptr, 12) as i64;
			match at(&str, ptr) {
				b'Y' => s.period.y = nr,
				b'W' => s.period.d = s.period.d.wrapping_add(nr.wrapping_mul(7)),
				b'D' => s.period.d = s.period.d.wrapping_add(nr),
				b'H' => s.period.h = nr,
				b'S' => s.period.s = nr,
				b'M' => {
					if in_time {
						s.period.i = nr;
					} else {
						s.period.m = nr;
					}
				}
				_ => s.add_error("Undefined period specifier"),
			}
			ptr += 1;
			if !(s.errors.error_count() == 0 && at(&str, ptr) != 0) {
				break;
			}
		}
		s.have_period = true;
		return TIMELIB_PERIOD;
	}

	combinedrep
	{
		init!(s, cursor, str, ptr);
		s.period.y = get_unsigned_nr(&str, &mut ptr, 4) as i64;
		ptr += 1;
		s.period.m = get_unsigned_nr(&str, &mut ptr, 2) as i64;
		ptr += 1;
		s.period.d = get_unsigned_nr(&str, &mut ptr, 2) as i64;
		ptr += 1;
		s.period.h = get_unsigned_nr(&str, &mut ptr, 2) as i64;
		ptr += 1;
		s.period.i = get_unsigned_nr(&str, &mut ptr, 2) as i64;
		ptr += 1;
		s.period.s = get_unsigned_nr(&str, &mut ptr, 2) as i64;
		s.have_period = true;
		return TIMELIB_PERIOD;
	}

	[ .,\t/]
	{
		continue 'std;
	}

	"\000"|"\n"
	{
		continue 'std;
	}

	any
	{
		s.add_error("Unexpected character");
		continue 'std;
	}
*/
