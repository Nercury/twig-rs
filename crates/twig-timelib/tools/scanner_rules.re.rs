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

	'yesterday'
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;
		unhave_time(&mut s.time);

		s.time.relative.d = -1;
		return TIMELIB_RELATIVE;
	}

	'now'
	{
		init!(s, cursor, str, ptr);

		return TIMELIB_RELATIVE;
	}

	'noon'
	{
		init!(s, cursor, str, ptr);
		unhave_time(&mut s.time);
		have_time!(s);
		s.time.h = 12;

		return TIMELIB_RELATIVE;
	}

	'midnight' | 'today'
	{
		init!(s, cursor, str, ptr);
		unhave_time(&mut s.time);

		return TIMELIB_RELATIVE;
	}

	'tomorrow'
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;
		unhave_time(&mut s.time);

		s.time.relative.d = 1;
		return TIMELIB_RELATIVE;
	}

	timestamp
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;
		unhave_date(&mut s.time);
		unhave_time(&mut s.time);
		have_tz!(s, cursor);

		let i = get_signed_nr(s, &str, &mut ptr, 24);
		s.time.y = 1970;
		s.time.m = 1;
		s.time.d = 1;
		s.time.h = 0;
		s.time.i = 0;
		s.time.s = 0;
		s.time.us = 0;
		s.time.relative.s = s.time.relative.s.wrapping_add(i);
		s.time.is_localtime = 1;
		s.time.zone_type = ZONETYPE_OFFSET;
		s.time.z = 0;
		s.time.dst = 0;

		return TIMELIB_RELATIVE;
	}

	timestampms
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;
		unhave_date(&mut s.time);
		unhave_time(&mut s.time);
		have_tz!(s, cursor);

		let is_negative = at(&str, ptr + 1) == b'-';

		let i = get_signed_nr(s, &str, &mut ptr, 24);

		let ptr_before = ptr;
		let mut us = get_signed_nr(s, &str, &mut ptr, 6) as u64;
		us = f64_to_ull(us as f64 * pow10(7 - (ptr - ptr_before) as i64));
		if is_negative {
			us = us.wrapping_neg();
		}

		s.time.y = 1970;
		s.time.m = 1;
		s.time.d = 1;
		s.time.h = 0;
		s.time.i = 0;
		s.time.s = 0;
		s.time.us = 0;
		s.time.relative.s = s.time.relative.s.wrapping_add(i);
		s.time.relative.us = us as i64;
		s.time.is_localtime = 1;
		s.time.zone_type = ZONETYPE_OFFSET;
		s.time.z = 0;
		s.time.dst = 0;

		return TIMELIB_RELATIVE;
	}

	firstdayof | lastdayof
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;

		/* skip "last day of" or "first day of" */
		if at(&str, ptr) == b'l' || at(&str, ptr) == b'L' {
			s.time.relative.first_last_day_of = SPECIAL_LAST_DAY_OF_MONTH;
		} else {
			s.time.relative.first_last_day_of = SPECIAL_FIRST_DAY_OF_MONTH;
		}

		return TIMELIB_LF_DAY_OF_MONTH;
	}

	backof | frontof
	{
		init!(s, cursor, str, ptr);
		unhave_time(&mut s.time);
		have_time!(s);

		if at(&str, ptr) == b'b' {
			s.time.h = get_nr(&str, &mut ptr, 2);
			s.time.i = 15;
		} else {
			s.time.h = get_nr(&str, &mut ptr, 2).wrapping_sub(1);
			s.time.i = 45;
		}
		if at(&str, ptr) != 0 {
			eat_spaces(&str, &mut ptr);
			let h = s.time.h;
			s.time.h = h.wrapping_add(meridian(&str, &mut ptr, h));
		}

		return TIMELIB_LF_DAY_OF_MONTH;
	}

	weekdayof
	{
		let mut behavior = 0;
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;
		s.time.have_relative = 1;
		s.time.relative.have_special_relative = 1;

		let i = get_relative_text(&str, &mut ptr, &mut behavior);
		eat_spaces(&str, &mut ptr);
		if i > 0 { /* first, second... etc */
			s.time.relative.special.kind = SPECIAL_DAY_OF_WEEK_IN_MONTH;
			set_relative(&str, &mut ptr, i, 1, s, TIME_PART_DONT_KEEP);
		} else { /* last */
			s.time.relative.special.kind = SPECIAL_LAST_DAY_OF_WEEK_IN_MONTH;
			set_relative(&str, &mut ptr, i, behavior, s, TIME_PART_DONT_KEEP);
		}
		return TIMELIB_WEEK_DAY_OF_MONTH;
	}

	timetiny12 | timeshort12 | timelong12
	{
		init!(s, cursor, str, ptr);
		have_time!(s);
		s.time.h = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
			s.time.i = get_nr(&str, &mut ptr, 2);
			if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
				s.time.s = get_nr(&str, &mut ptr, 2);
			}
		}
		eat_spaces(&str, &mut ptr);
		let h = s.time.h;
		s.time.h = h.wrapping_add(meridian(&str, &mut ptr, h));
		return TIMELIB_TIME12;
	}

	mssqltime
	{
		init!(s, cursor, str, ptr);
		have_time!(s);
		s.time.h = get_nr(&str, &mut ptr, 2);
		s.time.i = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
			s.time.s = get_nr(&str, &mut ptr, 2);

			if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
				s.time.us = get_frac_nr(&str, &mut ptr);
			}
		}
		eat_spaces(&str, &mut ptr);
		let h = s.time.h;
		s.time.h = h.wrapping_add(meridian(&str, &mut ptr, h));
		return TIMELIB_TIME24_WITH_ZONE;
	}

	timetiny24 | timeshort24 | timelong24 /* | iso8601short | iso8601norm */ | iso8601long /*| iso8601shorttz | iso8601normtz | iso8601longtz*/
	{
		let mut tz_not_found = false;
		init!(s, cursor, str, ptr);
		have_time!(s);
		s.time.h = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
			s.time.i = get_nr(&str, &mut ptr, 2);
			if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
				s.time.s = get_nr(&str, &mut ptr, 2);

				if at(&str, ptr) == b'.' {
					s.time.us = get_frac_nr(&str, &mut ptr);
				}
			}
		}

		if at(&str, ptr) != 0 {
			parse_zone_into!(s, str, ptr, tz_not_found, tz_get_wrapper);
			if tz_not_found {
				s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
			}
		}
		return TIMELIB_TIME24_WITH_ZONE;
	}

	gnunocolon
	{
		init!(s, cursor, str, ptr);
		match s.time.have_time {
			0 => {
				s.time.h = get_nr(&str, &mut ptr, 2);
				s.time.i = get_nr(&str, &mut ptr, 2);
				s.time.s = 0;
			}
			1 => {
				s.time.y = get_nr(&str, &mut ptr, 4);
			}
			_ => {
				s.add_error(ERR_DOUBLE_TIME, "Double time specification");
				return TIMELIB_ERROR;
			}
		}
		s.time.have_time += 1;
		return TIMELIB_GNU_NOCOLON;
	}

	iso8601nocolon /*| iso8601nocolontz*/
	{
		let mut tz_not_found = false;
		init!(s, cursor, str, ptr);
		have_time!(s);
		s.time.h = get_nr(&str, &mut ptr, 2);
		s.time.i = get_nr(&str, &mut ptr, 2);
		s.time.s = get_nr(&str, &mut ptr, 2);

		if at(&str, ptr) != 0 {
			parse_zone_into!(s, str, ptr, tz_not_found, tz_get_wrapper);
			if tz_not_found {
				s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
			}
		}
		return TIMELIB_ISO_NOCOLON;
	}

	americanshort | american
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b'/' {
			s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
			process_year(&mut s.time.y, length);
		}
		return TIMELIB_AMERICAN;
	}

	iso8601date4 | iso8601dateslash | dateslash
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		let y = get_signed_nr(s, &str, &mut ptr, 4);
		s.time.y = y;
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		return TIMELIB_ISO_DATE;
	}

	iso8601date2
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		process_year(&mut s.time.y, length);
		return TIMELIB_ISO_DATE;
	}

	iso8601datex
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		let y = get_signed_nr(s, &str, &mut ptr, 19);
		s.time.y = y;
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		return TIMELIB_ISO_DATE;
	}

	gnudateshorter
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = 1;
		process_year(&mut s.time.y, length);
		return TIMELIB_ISO_DATE;
	}

	gnudateshort
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		process_year(&mut s.time.y, length);
		return TIMELIB_ISO_DATE;
	}

	datefull
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.d = get_nr(&str, &mut ptr, 2);
		skip_day_suffix(&str, &mut ptr);
		s.time.m = get_month(&str, &mut ptr);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		process_year(&mut s.time.y, length);
		return TIMELIB_DATE_FULL;
	}

	pointeddate4
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.d = get_nr(&str, &mut ptr, 2);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.y = get_nr(&str, &mut ptr, 4);
		return TIMELIB_DATE_FULL_POINTED;
	}

	pointeddate2
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.d = get_nr(&str, &mut ptr, 2);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.y = get_nr_ex(&str, &mut ptr, 2, &mut length);
		process_year(&mut s.time.y, length);
		return TIMELIB_DATE_FULL_POINTED;
	}

	datenoday
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = get_month(&str, &mut ptr);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.d = 1;
		process_year(&mut s.time.y, length);
		return TIMELIB_DATE_NO_DAY;
	}

	datenodayrev
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.m = get_month(&str, &mut ptr);
		s.time.d = 1;
		process_year(&mut s.time.y, length);
		return TIMELIB_DATE_NO_DAY;
	}

	datetextual | datenoyear
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = get_month(&str, &mut ptr);
		s.time.d = get_nr(&str, &mut ptr, 2);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		process_year(&mut s.time.y, length);
		return TIMELIB_DATE_TEXT;
	}

	datenoyearrev
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.d = get_nr(&str, &mut ptr, 2);
		skip_day_suffix(&str, &mut ptr);
		s.time.m = get_month(&str, &mut ptr);
		return TIMELIB_DATE_TEXT;
	}

	datenocolon
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr(&str, &mut ptr, 4);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		return TIMELIB_DATE_NOCOLON;
	}

	xmlrpc | xmlrpcnocolon | soap | wddx | exif
	{
		let mut tz_not_found = false;
		init!(s, cursor, str, ptr);
		have_time!(s);
		have_date!(s);
		s.time.y = get_nr(&str, &mut ptr, 4);
		s.time.m = get_nr(&str, &mut ptr, 2);
		s.time.d = get_nr(&str, &mut ptr, 2);
		s.time.h = get_nr(&str, &mut ptr, 2);
		s.time.i = get_nr(&str, &mut ptr, 2);
		s.time.s = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b'.' {
			s.time.us = get_frac_nr(&str, &mut ptr);
			if at(&str, ptr) != 0 { /* timezone is optional */
				parse_zone_into!(s, str, ptr, tz_not_found, tz_get_wrapper);
				if tz_not_found {
					s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
				}
			}
		}
		return TIMELIB_XMLRPC_SOAP;
	}

	pgydotd
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.d = get_nr(&str, &mut ptr, 3);
		s.time.m = 1;
		process_year(&mut s.time.y, length);
		return TIMELIB_PG_YEARDAY;
	}

	isoweekday
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.have_relative = 1;

		s.time.y = get_nr(&str, &mut ptr, 4);
		let w = get_nr(&str, &mut ptr, 2);
		let d = get_nr(&str, &mut ptr, 1);
		s.time.m = 1;
		s.time.d = 1;
		s.time.relative.d = daynr_from_weeknr(s.time.y, w, d);

		return TIMELIB_ISO_WEEK;
	}

	isoweek
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.have_relative = 1;

		s.time.y = get_nr(&str, &mut ptr, 4);
		let w = get_nr(&str, &mut ptr, 2);
		let d = 1;
		s.time.m = 1;
		s.time.d = 1;
		s.time.relative.d = daynr_from_weeknr(s.time.y, w, d);

		return TIMELIB_ISO_WEEK;
	}

	pgtextshort
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = get_month(&str, &mut ptr);
		s.time.d = get_nr(&str, &mut ptr, 2);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		process_year(&mut s.time.y, length);
		return TIMELIB_PG_TEXT;
	}

	pgtextreverse
	{
		let mut length = 0;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.y = get_nr_ex(&str, &mut ptr, 4, &mut length);
		s.time.m = get_month(&str, &mut ptr);
		s.time.d = get_nr(&str, &mut ptr, 2);
		process_year(&mut s.time.y, length);
		return TIMELIB_PG_TEXT;
	}

	clf
	{
		let mut tz_not_found = false;
		init!(s, cursor, str, ptr);
		have_time!(s);
		have_date!(s);
		s.time.d = get_nr(&str, &mut ptr, 2);
		s.time.m = get_month(&str, &mut ptr);
		s.time.y = get_nr(&str, &mut ptr, 4);
		s.time.h = get_nr(&str, &mut ptr, 2);
		s.time.i = get_nr(&str, &mut ptr, 2);
		s.time.s = get_nr(&str, &mut ptr, 2);

		eat_spaces(&str, &mut ptr);

		parse_zone_into!(s, str, ptr, tz_not_found, tz_get_wrapper);
		if tz_not_found {
			s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
		}
		return TIMELIB_CLF;
	}

	year4
	{
		init!(s, cursor, str, ptr);
		s.time.y = get_nr(&str, &mut ptr, 4);
		return TIMELIB_CLF;
	}

	ago
	{
		init!(s, cursor, str, ptr);
		s.time.relative.y = s.time.relative.y.wrapping_neg();
		s.time.relative.m = s.time.relative.m.wrapping_neg();
		s.time.relative.d = s.time.relative.d.wrapping_neg();
		s.time.relative.h = s.time.relative.h.wrapping_neg();
		s.time.relative.i = s.time.relative.i.wrapping_neg();
		s.time.relative.s = s.time.relative.s.wrapping_neg();
		s.time.relative.weekday = s.time.relative.weekday.wrapping_neg();
		if s.time.relative.weekday == 0 {
			s.time.relative.weekday = -7;
		}
		if s.time.relative.have_special_relative != 0 && s.time.relative.special.kind == SPECIAL_WEEKDAY {
			s.time.relative.special.amount = s.time.relative.special.amount.wrapping_neg();
		}
		return TIMELIB_AGO;
	}

	daytext
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;
		s.time.have_relative = 1;
		s.time.relative.have_weekday_relative = 1;
		unhave_time(&mut s.time);
		if let Some(relunit) = lookup_relunit(&str, &mut ptr) {
			s.time.relative.weekday = relunit.multiplier();
		}
		if s.time.relative.weekday_behavior != 2 {
			s.time.relative.weekday_behavior = 1;
		}

		return TIMELIB_WEEKDAY;
	}

	relativetextweek
	{
		let mut behavior = 0;
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;

		while at(&str, ptr) != 0 {
			let before = ptr;
			let i = get_relative_text(&str, &mut ptr, &mut behavior);
			eat_spaces(&str, &mut ptr);
			set_relative(&str, &mut ptr, i, behavior, s, TIME_PART_DONT_KEEP);
			s.time.relative.weekday_behavior = 2;

			/* to handle the format weekday + last/this/next week */
			if s.time.relative.have_weekday_relative == 0 {
				s.time.have_relative = 1;
				s.time.relative.have_weekday_relative = 1;
				s.time.relative.weekday = 1;
			}
			if ptr == before {
				break; /* timelib would loop forever; the regex makes this unreachable */
			}
		}
		return TIMELIB_RELATIVE;
	}

	relativetext
	{
		let mut behavior = 0;
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;

		while at(&str, ptr) != 0 {
			let before = ptr;
			let i = get_relative_text(&str, &mut ptr, &mut behavior);
			eat_spaces(&str, &mut ptr);
			set_relative(&str, &mut ptr, i, behavior, s, TIME_PART_DONT_KEEP);
			if ptr == before {
				break; /* timelib would loop forever; the regex makes this unreachable */
			}
		}
		return TIMELIB_RELATIVE;
	}

	monthfull | monthabbr
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = lookup_month(&str, &mut ptr);
		return TIMELIB_DATE_TEXT;
	}

	tzcorrection | tz
	{
		let mut tz_not_found = false;
		init!(s, cursor, str, ptr);
		have_tz!(s, cursor);
		eat_spaces(&str, &mut ptr);
		parse_zone_into!(s, str, ptr, tz_not_found, tz_get_wrapper);
		if tz_not_found {
			s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
		}
		return TIMELIB_TIMEZONE;
	}

	dateshortwithtimeshort12 | dateshortwithtimelong12
	{
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = get_month(&str, &mut ptr);
		s.time.d = get_nr(&str, &mut ptr, 2);

		have_time!(s);
		s.time.h = get_nr(&str, &mut ptr, 2);
		s.time.i = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b':' || at(&str, ptr) == b'.' {
			s.time.s = get_nr(&str, &mut ptr, 2);

			if at(&str, ptr) == b'.' {
				s.time.us = get_frac_nr(&str, &mut ptr);
			}
		}

		let h = s.time.h;
		s.time.h = h.wrapping_add(meridian(&str, &mut ptr, h));
		return TIMELIB_SHORTDATE_WITH_TIME;
	}

	dateshortwithtimeshort | dateshortwithtimelong | dateshortwithtimelongtz
	{
		let mut tz_not_found = false;
		init!(s, cursor, str, ptr);
		have_date!(s);
		s.time.m = get_month(&str, &mut ptr);
		s.time.d = get_nr(&str, &mut ptr, 2);

		have_time!(s);
		s.time.h = get_nr(&str, &mut ptr, 2);
		s.time.i = get_nr(&str, &mut ptr, 2);
		if at(&str, ptr) == b':' {
			s.time.s = get_nr(&str, &mut ptr, 2);

			if at(&str, ptr) == b'.' {
				s.time.us = get_frac_nr(&str, &mut ptr);
			}
		}

		if at(&str, ptr) != 0 {
			parse_zone_into!(s, str, ptr, tz_not_found, tz_get_wrapper);
			if tz_not_found {
				s.add_error(ERR_TZID_NOT_FOUND, "The timezone could not be found in the database");
			}
		}
		return TIMELIB_SHORTDATE_WITH_TIME;
	}

	relative
	{
		init!(s, cursor, str, ptr);
		s.time.have_relative = 1;

		while at(&str, ptr) != 0 {
			let before = ptr;
			let i = get_signed_nr(s, &str, &mut ptr, 24);
			eat_spaces(&str, &mut ptr);
			set_relative(&str, &mut ptr, i, 1, s, TIME_PART_KEEP);
			if ptr == before {
				break; /* timelib would loop forever; the regex makes this unreachable */
			}
		}
		return TIMELIB_RELATIVE;
	}

	[.,]
	{
		continue 'std;
	}

	space
	{
		continue 'std;
	}

	"\000"|"\n"
	{
		continue 'std;
	}

	any
	{
		s.add_error(ERR_UNEXPECTED_CHARACTER, "Unexpected character");
		continue 'std;
	}
*/
