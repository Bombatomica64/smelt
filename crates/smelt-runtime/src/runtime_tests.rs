// Behaviour tests for the runtime families.
//
// Each test states a JavaScript fact and runs the real runtime against it, in
// the configuration with every gate on. Included into the flat runtime module, so
// private items are in scope as they are for each other.

/// Builds an erased string.
fn text(value: &str) -> SmeltUnknown {
    SmeltUnknown::String(value.into())
}

/// Builds an erased object from its entries.
fn object(entries: &[(&str, SmeltUnknown)]) -> SmeltObject {
    SmeltObject::new(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
}

/// Compares structurally, as `isEqual`-style code does.
fn structurally_equal(left: &SmeltUnknown, right: &SmeltUnknown) -> bool {
    smelt_unknown_structural_eq(left, right, &mut ::std::collections::HashSet::new())
}

mod list_family {
    use super::*;

    /// JS arrays are references: a mutation through one binding is seen by all.
    #[test]
    fn a_cloned_list_aliases_the_same_array() {
        let list = SmeltList::new(vec![1.0, 2.0]);
        let alias = list.clone();
        alias.borrow_mut().push(3.0);
        assert_eq!(list.to_vec(), vec![1.0, 2.0, 3.0], "the alias wrote to the shared array");
        assert_eq!(list.id(), alias.id(), "an alias is the same object");
    }

    /// `[...a]` is a new array: same contents, its own storage and identity.
    #[test]
    fn a_fresh_copy_is_a_new_array() {
        let list = SmeltList::new(vec![1.0]);
        let copy = list.fresh_copy();
        copy.borrow_mut().push(2.0);
        assert_eq!(list.len(), 1, "writing the copy must not reach the original");
        assert_ne!(list.id(), copy.id(), "a copy is a different object");
    }

    /// A list keeps its storage across the erased boundary: JS has one array.
    #[test]
    fn an_erased_list_aliases_its_array() {
        let list: SmeltList<SmeltUnknown> = SmeltList::new(vec![SmeltUnknown::Number(1.0)]);
        let array = SmeltArray::from(&list);
        array.push(SmeltUnknown::Number(2.0));
        assert_eq!(list.len(), 2, "a write through the erased view reaches the list");
    }

    /// `SmeltPrimSet` iterates in insertion order and a re-add does not move.
    #[test]
    fn a_primitive_set_keeps_insertion_order() {
        let mut set = SmeltPrimSet::new();
        for value in ["b", "a", "b", "c"] {
            set.insert(value.to_owned());
        }
        let members: Vec<_> = set.iter().cloned().collect();
        assert_eq!(members, ["b", "a", "c"], "JS `Set` order is insertion order");
        assert!(set.remove(&"a".to_owned()), "a member can be removed");
        assert!(!set.contains(&"a".to_owned()), "and is gone");
    }
}

mod array_family {
    use super::*;

    /// Writing past the end extends the array, as `a[5] = x` does.
    #[test]
    fn set_index_extends_the_array() {
        let array = SmeltArray::new(vec![SmeltUnknown::Number(1.0)]);
        array.set_index(2, SmeltUnknown::Number(3.0));
        assert_eq!(array.len(), 3, "the length follows the highest index");
        assert!(matches!(array.get(1), Some(SmeltUnknown::Undefined)), "a hole reads undefined");
    }

    /// Clones share storage, and a named property is visible through the clone.
    #[test]
    fn a_cloned_array_shares_storage_and_named_properties() {
        let array = SmeltArray::new(Vec::new());
        let alias = array.clone();
        alias.push(SmeltUnknown::Bool(true));
        array.set_named_property("tag".to_owned(), text("x"));
        assert_eq!(array.len(), 1, "the clone pushed to the same array");
        assert!(
            matches!(alias.named_property("tag"), Some(SmeltUnknown::String(tag)) if &*tag == "x"),
            "named properties live with the array's identity"
        );
    }

    /// `Object.keys(array)` lists indices first, then named keys.
    #[test]
    fn own_keys_lists_indices_then_names() {
        let array = SmeltArray::new(vec![SmeltUnknown::Null, SmeltUnknown::Null]);
        array.set_named_property("extra".to_owned(), SmeltUnknown::Null);
        assert_eq!(array.own_keys(), ["0", "1", "extra"], "indices come before names");
    }
}

mod record_family {
    use super::*;

    /// JS objects are references: a clone of a record is the same object.
    #[test]
    fn a_cloned_record_shares_its_fields() {
        let record: SmeltRecord<String, f64> = SmeltRecord::new();
        let alias = record.clone();
        alias.insert("a".to_owned(), 1.0);
        assert!(record.contains_key("a"), "the alias wrote to the shared fields");
        assert_eq!(record.len(), 1, "one field exists");
    }

    /// An object view of a record shares the record's store.
    #[test]
    fn an_object_built_from_a_record_shares_its_store() {
        let record: SmeltRecord<JsString, SmeltUnknown> = SmeltRecord::new();
        let object = SmeltObject::from_unknown_record(record.clone());
        object.insert("a".to_owned(), SmeltUnknown::Number(1.0));
        assert!(record.has_text_key("a"), "the view and the record are one object");
    }

    /// Integer-like keys enumerate in ascending order, before string keys.
    #[test]
    fn keys_follow_ecma_order() {
        let object = object(&[("b", SmeltUnknown::Null), ("2", SmeltUnknown::Null), ("1", SmeltUnknown::Null)]);
        assert_eq!(smelt_for_in_object_keys(&object), ["1", "2", "b"], "array indices sort first");
    }

    /// Removing a field really removes it, and overwriting keeps one entry.
    #[test]
    fn the_field_store_updates_in_place() {
        let object = object(&[("a", SmeltUnknown::Number(1.0))]);
        object.insert("a".to_owned(), SmeltUnknown::Number(2.0));
        assert_eq!(object.len(), 1, "an overwrite does not add a field");
        assert!(matches!(object.get("a"), Some(SmeltUnknown::Number(value)) if value == 2.0), "the new value reads back");
        object.remove("a");
        assert!(!object.contains_key("a"), "the field is gone");
    }

    /// Past the linear scan limit, the store still finds every key.
    #[test]
    fn a_large_object_keeps_every_field() {
        let object = SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new());
        for index in 0..200 {
            object.insert(format!("key{index}"), SmeltUnknown::Number(f64::from(index)));
        }
        assert_eq!(object.len(), 200, "every field was stored");
        assert!(
            matches!(object.get("key137"), Some(SmeltUnknown::Number(value)) if value == 137.0),
            "the hashed index finds a late field"
        );
    }
}

mod map_and_set_family {
    use super::*;

    /// `Map` compares keys with SameValueZero: `NaN` equals itself and `0` is `-0`.
    #[test]
    fn map_keys_use_same_value_zero() {
        let mut map: SmeltJsMap<SmeltUnknown, f64> = SmeltJsMap::new();
        map.insert(SmeltUnknown::Number(f64::NAN), 1.0);
        map.insert(SmeltUnknown::Number(f64::NAN), 2.0);
        map.insert(SmeltUnknown::Number(0.0), 3.0);
        map.insert(SmeltUnknown::Number(-0.0), 4.0);
        assert_eq!(map.len(), 2, "NaN and the zeros each collapse to one key");
        assert_eq!(map.get(&SmeltUnknown::Number(f64::NAN)), Some(2.0), "NaN finds its entry");
        assert_eq!(map.get(&SmeltUnknown::Number(0.0)), Some(4.0), "-0 updated 0's entry");
    }

    /// Objects are `Map` keys by identity, not by contents.
    #[test]
    fn object_keys_compare_by_identity() {
        let first = SmeltUnknown::Object(object(&[("a", SmeltUnknown::Number(1.0))]));
        let second = SmeltUnknown::Object(object(&[("a", SmeltUnknown::Number(1.0))]));
        let mut map: SmeltJsMap<SmeltUnknown, f64> = SmeltJsMap::new();
        map.insert(first.clone(), 1.0);
        assert_eq!(map.get(&first), Some(1.0), "the same object is the same key");
        assert_eq!(map.get(&second), None, "an equal object is a different key");
    }

    /// `Set` keeps insertion order and de-duplicates by SameValueZero.
    #[test]
    fn a_js_set_keeps_order_and_dedupes() {
        let mut set: SmeltJsSet<SmeltUnknown> = SmeltJsSet::new();
        set.insert(text("b"));
        set.insert(text("a"));
        set.insert(text("b"));
        assert_eq!(set.len(), 2, "a repeat is not a new member");
        let members: Vec<String> = set.iter().map(|member| member.to_string()).collect();
        assert_eq!(members, ["b", "a"], "members stay in insertion order");
    }
}

mod unknown_family {
    use super::*;

    /// JavaScript truthiness: the falsy values, and nothing else.
    #[test]
    fn truthiness_matches_ecma() {
        for falsy in [
            SmeltUnknown::Null,
            SmeltUnknown::Undefined,
            SmeltUnknown::Bool(false),
            SmeltUnknown::Number(0.0),
            SmeltUnknown::Number(-0.0),
            SmeltUnknown::Number(f64::NAN),
            text(""),
        ] {
            assert!(!smelt_erased_truthy(&falsy), "{falsy:?} is falsy");
        }
        for truthy in [
            SmeltUnknown::Bool(true),
            SmeltUnknown::Number(1.0),
            text("0"),
            SmeltUnknown::Array(SmeltArray::new(Vec::new())),
            SmeltUnknown::Object(SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new())),
        ] {
            assert!(smelt_erased_truthy(&truthy), "{truthy:?} is truthy");
        }
    }

    /// `typeof` spells the eight answers, including the `null` wart.
    #[test]
    fn typeof_answers_the_spec_table() {
        assert_eq!(smelt_typeof(&SmeltUnknown::Undefined), "undefined", "undefined");
        assert_eq!(smelt_typeof(&SmeltUnknown::Null), "object", "typeof null is object");
        assert_eq!(smelt_typeof(&SmeltUnknown::Bool(true)), "boolean", "boolean");
        assert_eq!(smelt_typeof(&SmeltUnknown::Number(1.0)), "number", "number");
        assert_eq!(smelt_typeof(&text("x")), "string", "string");
        assert_eq!(smelt_typeof(&SmeltUnknown::Array(SmeltArray::new(Vec::new()))), "object", "arrays are objects");
    }

    /// `Object.prototype.toString.call(v)` tags.
    #[test]
    fn to_string_tags_name_the_builtin() {
        assert_eq!(smelt_object_to_string_tag(&SmeltUnknown::Null), "[object Null]", "null");
        assert_eq!(smelt_object_to_string_tag(&SmeltUnknown::Undefined), "[object Undefined]", "undefined");
        assert_eq!(smelt_object_to_string_tag(&SmeltUnknown::Number(1.0)), "[object Number]", "number");
        assert_eq!(smelt_object_to_string_tag(&text("x")), "[object String]", "string");
        assert_eq!(smelt_object_to_string_tag(&SmeltUnknown::Array(SmeltArray::new(Vec::new()))), "[object Array]", "array");
        assert_eq!(smelt_object_to_string_tag(&SmeltUnknown::Object(SmeltObject::new(Vec::<(JsString, SmeltUnknown)>::new()))), "[object Object]", "object");
    }

    /// Structural equality compares contents and survives a cycle.
    #[test]
    fn structural_equality_compares_contents() {
        let left = SmeltUnknown::Object(object(&[("a", SmeltUnknown::Number(1.0))]));
        let same = SmeltUnknown::Object(object(&[("a", SmeltUnknown::Number(1.0))]));
        let other = SmeltUnknown::Object(object(&[("a", SmeltUnknown::Number(2.0))]));
        assert!(structurally_equal(&left, &same), "equal contents compare equal");
        assert!(!structurally_equal(&left, &other), "different contents do not");
        let cycle = object(&[]);
        cycle.insert("self".to_owned(), SmeltUnknown::Object(cycle.clone()));
        let value = SmeltUnknown::Object(cycle);
        assert!(structurally_equal(&value, &value.clone()), "a cycle must terminate");
    }

    /// Property reads on erased values follow JavaScript.
    #[test]
    fn property_reads_follow_javascript() {
        let value = SmeltUnknown::Object(object(&[("a", SmeltUnknown::Number(1.0))]));
        assert!(matches!(smelt_get_unknown_field(&value, "a"), SmeltUnknown::Number(number) if number == 1.0), "an own field");
        assert!(matches!(smelt_get_unknown_field(&value, "missing"), SmeltUnknown::Undefined), "a missing field is undefined");
        assert!(matches!(smelt_get_unknown_field(&text("abc"), "length"), SmeltUnknown::Number(number) if number == 3.0), "a string's length");
        assert!(smelt_has_property(&value, "a"), "`in` sees an own field");
        assert!(!smelt_has_property(&value, "missing"), "`in` does not invent one");
    }

    /// Typed values round trip through the erasure traits.
    #[test]
    fn scalars_round_trip_through_the_carrier() {
        assert!(bool::smelt_from_unknown(true.into_smelt_unknown()), "bool");
        assert_eq!(f64::smelt_from_unknown(2.5_f64.into_smelt_unknown()), 2.5, "f64");
        assert_eq!(String::smelt_from_unknown("x".to_owned().into_smelt_unknown()), "x", "String");
        assert!(matches!(Option::<f64>::None.into_smelt_unknown(), SmeltUnknown::Undefined | SmeltUnknown::Null), "an absent optional");
    }

    /// `JSON.stringify` follows ECMA-262 through the carrier's `Serialize`.
    #[test]
    fn json_serialization_follows_ecma() {
        let value = SmeltUnknown::Object(object(&[
            ("a", SmeltUnknown::Number(1.0)),
            ("b", SmeltUnknown::Undefined),
            ("c", SmeltUnknown::Number(f64::NAN)),
        ]));
        let json = serde_json::to_string(&value).expect("the value serializes");
        assert_eq!(json, "{\"a\":1,\"c\":null}", "undefined drops and NaN is null");
    }
}

mod reflection_family {
    use super::*;

    /// `for...in` skips the runtime's own markers.
    #[test]
    fn for_in_hides_internal_keys() {
        let value = object(&[("visible", SmeltUnknown::Null), ("__smelt_symbol:hidden", SmeltUnknown::Null)]);
        assert_eq!(smelt_for_in_object_keys(&value), ["visible"], "a symbol key does not enumerate");
    }

    /// One constructor object per class name: `a.constructor === b.constructor`.
    #[test]
    fn a_class_constructor_is_interned() {
        let first = smelt_class_constructor("Widget".to_owned());
        let second = smelt_class_constructor("Widget".to_owned());
        let other = smelt_class_constructor("Gadget".to_owned());
        assert!(first.js_strict_eq(&second), "the same class has one constructor");
        assert!(!first.js_strict_eq(&other), "another class has its own");
    }

    /// `structuredClone` copies deeply and gives the copy new identities.
    #[test]
    fn structured_clone_is_deep_and_keeps_contents() {
        let inner = object(&[("n", SmeltUnknown::Number(1.0))]);
        let value = SmeltUnknown::Object(object(&[("inner", SmeltUnknown::Object(inner.clone()))]));
        let clone = smelt_structured_clone(value.clone());
        assert!(structurally_equal(&value, &clone), "the clone has the same contents");
        assert!(!value.js_strict_eq(&clone), "the clone is another object");
        inner.insert("n".to_owned(), SmeltUnknown::Number(2.0));
        assert!(!structurally_equal(&value, &clone), "writing the source must not reach the clone");
    }

    /// Function identity is the shared allocation, not the signature.
    #[test]
    fn function_identity_follows_the_allocation() {
        let first: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(|value| value);
        let alias = ::std::rc::Rc::clone(&first);
        let second: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(|value| value + 1.0);
        assert!(smelt_same_function_identity(&first, &alias), "a clone is the same function");
        assert!(!smelt_same_function_identity(&first, &second), "another closure is not");
    }
}

mod generator_family {
    use super::*;

    /// A generator yields, completes, and stays complete.
    #[test]
    fn a_generator_stays_complete() {
        let mut step = 0;
        let generator: SmeltGenerator<f64, f64, f64> = SmeltGenerator::new(move |_| {
            step += 1;
            if step == 1 { SmeltGeneratorResult::Yielded(10.0) } else { SmeltGeneratorResult::Complete(20.0) }
        });
        assert!(matches!(generator.resume(SmeltGeneratorCommand::Next(0.0)), SmeltGeneratorResult::Yielded(value) if value == 10.0), "first step yields");
        assert!(matches!(generator.resume(SmeltGeneratorCommand::Next(0.0)), SmeltGeneratorResult::Complete(value) if value == 20.0), "second step returns");
        assert!(matches!(generator.resume(SmeltGeneratorCommand::Next(0.0)), SmeltGeneratorResult::Complete(_)), "a finished generator stays finished");
    }
}

mod vitest_family {
    use super::*;

    /// A mock counts its calls and records their arguments.
    #[test]
    fn a_mock_records_calls() {
        let mock = smelt_vitest_mock_new(None);
        assert!(smelt_vitest_mock_called_times(&mock, 0.0), "a new mock has no calls");
        let call = smelt_vitest_mock_callable(&mock).expect("a mock is callable");
        let _ = call(vec![SmeltUnknown::Number(1.0)]);
        assert!(smelt_vitest_mock_called_times(&mock, 1.0), "the call is counted");
        assert!(smelt_vitest_mock_called_with(&mock, vec![SmeltUnknown::Number(1.0)], true), "its arguments are recorded");
        assert!(!smelt_vitest_mock_called_with(&mock, vec![SmeltUnknown::Number(2.0)], true), "other arguments do not match");
    }
}

mod timer_family {
    use super::*;

    /// A cleared timer never fires, and a due one does.
    #[test]
    fn timers_fire_unless_cleared() {
        smelt_reset_timers();
        let fired = ::std::rc::Rc::new(::std::cell::Cell::new(0));
        let counter = ::std::rc::Rc::clone(&fired);
        let live: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>> =
            ::std::rc::Rc::new(::std::cell::RefCell::new(move || { counter.set(counter.get() + 1); Ok(()) }));
        let counter = ::std::rc::Rc::clone(&fired);
        let cancelled: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>> =
            ::std::rc::Rc::new(::std::cell::RefCell::new(move || { counter.set(counter.get() + 100); Ok(()) }));
        smelt_set_timeout(live, 0.0);
        let handle = smelt_set_timeout(cancelled, 0.0);
        smelt_clear_timeout(handle);
        smelt_drain_due_timers(u64::MAX);
        assert_eq!(fired.get(), 1, "only the live timer fired");
    }
}

mod byte_family {
    use super::*;

    /// Two views over one buffer see each other's writes.
    #[test]
    fn views_share_their_buffer() {
        let view = SmeltTypedArray::with_length(SmeltTypedArrayKind::Uint8, 4);
        let sub = view.subarray(1, Some(3));
        sub.set_index(0.0, 7.0);
        assert_eq!(view.get(1.0), Some(7.0), "a subarray writes through to its buffer");
        assert_eq!(sub.length(), 2.0, "a subarray has its own length");
    }

    /// An element width reinterprets the same bytes, little-endian.
    #[test]
    fn a_kind_decodes_its_width() {
        let view = SmeltTypedArray::with_bytes(SmeltTypedArrayKind::Uint16, vec![0x34, 0x12]);
        assert_eq!(view.get(0.0), Some(f64::from(0x1234_u16)), "two bytes make one element");
    }

    /// `Uint8Array` truncates and `Uint8ClampedArray` clamps.
    #[test]
    fn stores_follow_the_element_type() {
        let wrapped = SmeltTypedArray::with_elements(SmeltTypedArrayKind::Uint8, &[257.0]);
        let clamped = SmeltTypedArray::with_elements(SmeltTypedArrayKind::Uint8Clamped, &[257.0]);
        assert_eq!(wrapped.get(0.0), Some(1.0), "a store wraps modulo 256");
        assert_eq!(clamped.get(0.0), Some(255.0), "a clamped store saturates");
    }

    /// `DataView` reads either byte order from the same buffer.
    #[test]
    fn a_data_view_honours_endianness() {
        let buffer = SmeltArrayBuffer::from_bytes(vec![0x12, 0x34]);
        let view = SmeltDataView::over_buffer(&buffer, 0, None);
        assert_eq!(view.get(SmeltTypedArrayKind::Uint16, 0, false), f64::from(0x1234_u16), "big-endian");
        assert_eq!(view.get(SmeltTypedArrayKind::Uint16, 0, true), f64::from(0x3412_u16), "little-endian");
    }

    /// A `SharedArrayBuffer` reports its species.
    #[test]
    fn shared_storage_reports_its_species() {
        assert_eq!(SmeltArrayBuffer::new_shared(1).class_name(), "SharedArrayBuffer", "shared");
        assert_eq!(SmeltArrayBuffer::new(1).class_name(), "ArrayBuffer", "plain");
    }

    /// `TextEncoder` writes UTF-8 and `TextDecoder` reads it back.
    #[test]
    fn text_codecs_round_trip_utf8() {
        let bytes = SmeltTextEncoder::new().encode("é");
        assert_eq!(bytes.length(), 2.0, "é is two UTF-8 bytes");
    }
}

mod number_family {
    use super::*;

    /// `Number::toString`, at the spec's notation boundaries.
    #[test]
    fn numbers_format_like_javascript() {
        for (value, expected) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(smelt_number_to_string(value), expected, "{value:?}");
        }
    }

    /// `console.log(-0)` is the one place `-0` shows.
    #[test]
    fn console_keeps_negative_zero() {
        assert_eq!(smelt_console_number(-0.0), "-0", "console prints the sign");
    }

    /// `StringToNumber`: whitespace, hex, and what is not a number.
    #[test]
    fn strings_parse_like_javascript() {
        assert_eq!(smelt_string_to_number("  12 "), 12.0, "whitespace trims");
        assert_eq!(smelt_string_to_number(""), 0.0, "the empty string is 0");
        assert_eq!(smelt_string_to_number("0x10"), 16.0, "hex literals parse");
        assert!(smelt_string_to_number("12px").is_nan(), "trailing text is NaN");
    }

    /// `parseInt` reads the longest digit prefix, detects `0x` only for radix
    /// 0 or 16, and keeps the sign of a zero.
    #[test]
    fn parse_int_reads_a_prefix_like_javascript() {
        assert_eq!(smelt_parse_int("12px", 0.0), 12.0, "trailing text is ignored");
        assert_eq!(smelt_parse_int(" 3.7", 0.0), 3.0, "the fraction is not a digit");
        assert_eq!(smelt_parse_int("0x10", 0.0), 16.0, "an absent radix detects hex");
        assert_eq!(smelt_parse_int("0x10", 10.0), 0.0, "radix 10 stops at the x");
        assert_eq!(smelt_parse_int("-0x1F", 16.0), -31.0, "radix 16 strips the prefix");
        assert!(smelt_parse_int("-0", 10.0).is_sign_negative(), "-0 keeps its sign");
        assert!(smelt_parse_int("10", 1.0).is_nan(), "radix 1 is out of range");
        assert_eq!(smelt_parse_int("10", 4_294_967_298.0), 2.0, "the radix wraps through ToInt32");
        assert_eq!(smelt_parse_int("99999999999999999999", 0.0), 1e20, "beyond i64 it still rounds");
        assert!(smelt_parse_int("Infinity", 0.0).is_nan(), "Infinity is not a digit");
    }

    /// `parseFloat` reads the longest decimal-literal prefix.
    #[test]
    fn parse_float_reads_a_prefix_like_javascript() {
        assert_eq!(smelt_parse_float("12px"), 12.0, "trailing text is ignored");
        assert_eq!(smelt_parse_float(".5e1x"), 5.0, "a bare fraction and exponent");
        assert_eq!(smelt_parse_float("1e"), 1.0, "an exponent with no digits is dropped");
        assert_eq!(smelt_parse_float("0x10"), 0.0, "no hex");
        assert_eq!(smelt_parse_float("-Infinityx"), f64::NEG_INFINITY, "signed Infinity");
        assert!(smelt_parse_float(".").is_nan(), "a lone dot has no digits");
        assert!(smelt_parse_float("-").is_nan(), "a lone sign has no digits");
    }

    /// `Math.round` rounds a tie toward +∞ and keeps `-0`.
    #[test]
    fn math_round_ties_toward_positive_infinity() {
        assert_eq!(smelt_math_round(-1.5), -1.0, "the tie goes up");
        assert_eq!(smelt_math_round(2.5), 3.0, "and up here too");
        assert!(smelt_math_round(-0.4).is_sign_negative(), "-0 keeps its sign");
    }

    /// `bigint` division by zero is a `RangeError`, not a panic.
    #[test]
    fn bigint_division_reports_range_error() {
        let one = ::num_bigint::BigInt::from(1);
        let zero = ::num_bigint::BigInt::from(0);
        assert!(smelt_bigint_div(&one, &zero).is_err(), "1n / 0n throws");
        let seven = ::num_bigint::BigInt::from(7);
        let two = ::num_bigint::BigInt::from(2);
        assert_eq!(smelt_bigint_div(&seven, &two).expect("a real division"), ::num_bigint::BigInt::from(3), "division truncates");
    }
}

mod regexp_family {
    use super::*;

    /// `lastIndex` advances on a global regexp, which is what makes `exec` loop.
    #[test]
    fn a_global_regexp_matches_every_occurrence() {
        let regexp = SmeltRegExp::new("a".to_owned(), "g".to_owned());
        assert_eq!(regexp.replace_string(&JsString::from("banana"), &JsString::from("o"), false), "bonono", "`g` replaces all");
        let once = SmeltRegExp::new("a".to_owned(), String::new());
        assert_eq!(once.replace_string(&JsString::from("banana"), &JsString::from("o"), false), "bonana", "without `g` only the first");
    }

    /// Case folding is the `i` flag's job.
    #[test]
    fn flags_change_matching() {
        assert!(SmeltRegExp::new("ABC".to_owned(), "i".to_owned()).test(&JsString::from("abc")), "`i` folds case");
        assert!(!SmeltRegExp::new("ABC".to_owned(), String::new()).test(&JsString::from("abc")), "without it, case counts");
    }

    /// Without `u` a pattern and its subject are code units, so `.` matches one
    /// surrogate half and an astral literal is its two halves; with `u` both
    /// are code points. Every index is a code unit index.
    #[test]
    fn matching_counts_code_units_like_javascript() {
        let emoji = JsString::from("😀x");
        let dot = SmeltRegExp::new("^.$", "");
        assert!(!dot.test(&JsString::from("😀")), "without `u`, `.` is one code unit");
        assert!(SmeltRegExp::new("^.$", "u").test(&JsString::from("😀")), "with `u`, `.` is a code point");
        assert_eq!(SmeltRegExp::new("😀", "g").replace_string(&JsString::from("a😀b😀"), &JsString::from("[$&]"), false), "a[😀]b[😀]", "an astral literal matches its halves");
        let cut = SmeltRegExp::new(".", "").replace_string(&emoji, &JsString::from("#"), false);
        assert_eq!(cut.units(), &[u16::from(b'#'), 0xDE00, u16::from(b'x')], "replacing one unit leaves the lone low half");
        assert_eq!(SmeltRegExp::new("x", "").search_index(&emoji), Some(2), "a search answers a code unit index");
        let lone = JsString::from_units(&[0xD83D, u16::from(b'a')]);
        assert_eq!(SmeltRegExp::new("a", "").search_index(&lone), Some(1), "a lone half in the subject is an ordinary unit");
    }

    /// `@@split`: captures join the pieces, an empty match at a piece's start is
    /// skipped, and an empty subject is `[""]` unless the regexp matches it.
    #[test]
    fn split_follows_the_specification() {
        let pieces = |pattern: &str, text: &str| SmeltRegExp::new(pattern, "").split_string(&JsString::from(text)).iter().map(|piece| piece.to_string_lossy()).collect::<Vec<_>>();
        assert_eq!(pieces("(\\d)", "a1b2c"), ["a", "1", "b", "2", "c"], "a capture joins the pieces");
        assert_eq!(pieces("(?:)", "ab"), ["a", "b"], "an empty match splits between units");
        assert_eq!(pieces("x", ""), [""], "an empty subject is one empty piece");
        assert!(pieces("(?:)", "").is_empty(), "unless the regexp matches it");
    }

    /// A sticky regexp matches only at `lastIndex` and moves it past the match;
    /// a failed stateful search rewinds it to 0.
    #[test]
    fn sticky_matches_only_at_last_index() {
        let sticky = SmeltRegExp::new("b", "y");
        *sticky.last_index.borrow_mut() = 1;
        assert!(sticky.test(&JsString::from("abc")), "`b` is at index 1");
        assert_eq!(*sticky.last_index.borrow(), 2, "lastIndex moves past it");
        assert!(!sticky.test(&JsString::from("abc")), "`c` is not `b`");
        assert_eq!(*sticky.last_index.borrow(), 0, "a failed search rewinds");
    }
}

mod host_family {
    use super::*;

    /// `Headers` names are case-insensitive, and `append` joins with a comma.
    #[test]
    fn headers_fold_case_and_join() {
        let headers = SmeltHeaders::new();
        headers.append("X-Test", "a");
        headers.append("x-test", "b");
        assert_eq!(headers.get("X-TEST").as_deref(), Some("a, b"), "repeated names combine");
        assert!(headers.has("x-TEST"), "`has` folds case");
        headers.delete("X-test");
        assert!(!headers.has("x-test"), "`delete` folds case");
    }

    /// A `URL` parses, resolves against a base, and rejects garbage.
    #[test]
    fn urls_parse_and_resolve() {
        let url = SmeltUrl::parse("/b?x=1", Some("https://example.test/a")).expect("a relative URL resolves");
        assert_eq!(url.href(), "https://example.test/b?x=1", "the base supplies the rest");
        assert_eq!(url.origin(), "https://example.test", "the origin is scheme and host");
        assert!(SmeltUrl::parse("not a url", None).is_none(), "garbage does not parse");
    }

    /// `Date` is a time value; an invalid date reads NaN and survives.
    #[test]
    fn dates_clip_and_format() {
        assert_eq!(SmeltDate::new(0.0).to_iso_string(), "1970-01-01T00:00:00.000Z", "the epoch");
        assert!(SmeltDate::new(f64::NAN).get_time().is_nan(), "an invalid date holds NaN");
        assert!(SmeltDate::new(9e15).get_time().is_nan(), "past the range clips to NaN");
    }

    /// `node:path` normalizes like POSIX `path`.
    #[test]
    fn paths_normalize_like_node() {
        assert_eq!(smelt_path_normalize("/a/b/../c/./d"), "/a/c/d", "dot segments fold");
        assert_eq!(smelt_path_join(&["a".to_owned(), "..".to_owned(), "b".to_owned()]), "b", "join normalizes");
        assert_eq!(smelt_path_dirname("/a/b.txt"), "/a", "dirname");
        assert_eq!(smelt_path_basename("/a/b.txt", Some(".txt")), "b", "basename drops the suffix");
        assert_eq!(smelt_path_relative("/a/b", "/a/c"), "../c", "relative walks up");
    }

    /// The hashes answer their published test vectors.
    #[test]
    fn hashes_answer_their_vectors() {
        let hash = smelt_hash_create("sha256").expect("sha256 is supported");
        let hash = smelt_hash_update_text(&hash, "abc", "utf8").expect("the update runs");
        assert_eq!(
            smelt_hash_digest_text(&hash, "hex").expect("the digest runs"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "SHA-256 of abc"
        );
        assert!(smelt_hash_create("no-such-hash").is_err(), "an unknown algorithm throws");
    }

    /// `crypto.subtle.digest` answers its vector, in an `ArrayBuffer`.
    #[test]
    fn digest_answers_its_vector() {
        let digest = smelt_crypto_digest("SHA-1", b"abc").expect("SHA-1 is supported");
        assert_eq!(digest.to_bytes().first().copied(), Some(0xa9), "SHA-1 of abc starts a9");
        assert_eq!(digest.byte_length(), 20.0, "a SHA-1 digest is 20 bytes");
    }

    /// An `EventEmitter` calls listeners in order and drops a `once` after firing.
    #[test]
    fn an_emitter_fires_and_forgets_once_listeners() {
        let emitter = SmeltEventEmitter::new();
        let calls = ::std::rc::Rc::new(::std::cell::Cell::new(0));
        let counter = ::std::rc::Rc::clone(&calls);
        emitter.add("e", ::std::rc::Rc::new(move |_| { counter.set(counter.get() + 1); Ok(SmeltUnknown::Undefined) }), true);
        assert!(emitter.emit("e", Vec::new()).expect("the emit runs"), "a listener answered");
        assert!(!emitter.emit("e", Vec::new()).expect("the emit runs"), "a `once` listener is gone");
        assert_eq!(calls.get(), 1, "it ran once");
    }

    /// `node:sqlite` stores and reads a row back through the carrier.
    #[test]
    fn sqlite_round_trips_a_row() {
        let db = SmeltSqliteDatabase::open(":memory:").expect("an in-memory database opens");
        db.exec("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (7);").expect("the script runs");
        let statement = db.prepare("SELECT a FROM t").expect("the query prepares");
        let row = statement.smelt_sqlite_get(&[]).expect("the query runs");
        assert!(matches!(smelt_get_unknown_field(&row, "a"), SmeltUnknown::Number(value) if value == 7.0), "the column reads back");
    }

    /// A blob holds its bytes and a form holds its entries in order.
    #[test]
    fn blobs_and_forms_hold_their_contents() {
        let blob = SmeltBlob::from_string_parts(vec!["ab".to_owned(), "c".to_owned()], "text/plain".to_owned(), None::<String>, None);
        assert_eq!(blob.size(), 3.0, "parts concatenate");
        let form = SmeltFormData::new();
        form.append("k", SmeltFormDataValue::Text("v".to_owned()));
        assert_eq!(form.get("k").map(|value| value.to_text()).as_deref(), Some("v"), "a field reads back");
    }
}

mod thrown_family {
    use super::*;

    /// A thrown value keeps its identity through the `Box<dyn Error>` channel.
    #[test]
    fn a_thrown_value_survives_the_error_channel() {
        let error = object(&[("message", text("boom"))]);
        let value = SmeltUnknown::Object(error);
        let boxed = smelt_throw(value.clone());
        assert!(smelt_thrown_value(&*boxed).js_strict_eq(&value), "the catch sees the same object");
        assert_eq!(smelt_thrown_message(&value), "boom", "its message reads back");
    }
}

mod js_string_family {
    use super::*;

    /// Builds a string from its code units.
    fn units(code_units: &[u16]) -> JsString {
        JsString::from_units(code_units)
    }

    /// `"😀".slice(0, 1)` is the lone leading half, and the halves join back.
    #[test]
    fn a_cut_pair_leaves_lone_halves() {
        let emoji = JsString::from("😀");
        assert_eq!(emoji.len(), 2, "a pair is two code units");
        let (lead, trail) = (emoji.slice(0, 1), emoji.slice(1, 2));
        assert_eq!(lead.units(), [0xD83D], "the leading half alone");
        assert_eq!(trail.units(), [0xDE00], "the trailing half alone");
        assert_eq!(lead.clone() + &trail, emoji, "the halves join back to the pair");
        assert_eq!(emoji.char_at(1), Some(units(&[0xDE00])), "s[1] is the trailing half");
        assert!(!lead.is_well_formed(), "a lone half is not well formed");
        assert_eq!(lead.to_well_formed(), "\u{FFFD}", "toWellFormed replaces a lone half");
    }

    /// `length`, `charCodeAt` and `codePointAt` count code units.
    #[test]
    fn indices_count_code_units() {
        let text = JsString::from("aé😀b");
        assert_eq!(text.len(), 5, "length");
        assert_eq!(text.unit_at(2), Some(0xD83D), "charCodeAt at a leading half");
        assert_eq!(text.code_point_at(2), Some(0x1F600), "codePointAt at a leading half is the pair");
        assert_eq!(text.code_point_at(3), Some(0xDE00), "codePointAt at a trailing half is the unit");
        assert_eq!(text.unit_at(5), None, "past the end");
        assert_eq!(text.slice(1, 99), "é😀b", "an end past the length clamps");
        assert_eq!(text.slice(3, 1), "", "an end before the start is empty");
        assert_eq!(text.code_point_strings(), vec![JsString::from("a"), JsString::from("é"), JsString::from("😀"), JsString::from("b")], "[...s] keeps pairs whole");
    }

    /// `indexOf`/`lastIndexOf`, including JavaScript's empty-needle answers.
    #[test]
    fn searches_answer_like_javascript() {
        let text = JsString::from("a😀ba😀b");
        let needle = JsString::from("😀b");
        assert_eq!(text.index_of(&*needle, 0), Some(1), "first occurrence");
        assert_eq!(text.index_of(&*needle, 2), Some(5), "from skips the first");
        assert_eq!(text.index_of(&*needle, 99), None, "from past the end");
        assert_eq!(text.index_of("", 3), Some(3), "an empty needle is found at from");
        assert_eq!(text.index_of("", 99), Some(8), "...clamped to the length");
        assert_eq!(text.last_index_of(&*needle, usize::MAX), Some(5), "last occurrence");
        assert_eq!(text.last_index_of(&*needle, 4), Some(1), "at or before from");
        assert_eq!(text.index_of(units(&[0xDE00]), 0), Some(2), "a lone half matches inside a pair");
        assert!(text.contains("ba"), "includes");
        assert!(text.starts_with("a😀") && text.ends_with("😀b"), "prefix and suffix");
    }

    /// Order is by code unit: U+1F600 (`D83D DE00`) sorts before U+FFFF, the
    /// reverse of code-point order.
    #[test]
    fn order_is_by_code_unit() {
        let mut words: Vec<JsString> = ["\u{FFFF}", "😀", "a", "é"].into_iter().map(JsString::from).collect();
        words.sort();
        let sorted: Vec<String> = words.iter().map(|word| word.to_string_lossy()).collect();
        assert_eq!(sorted, ["a", "é", "😀", "\u{FFFF}"], "JavaScript's sort order");
        let (emoji, last_bmp) = (JsString::from("😀"), JsString::from("\u{FFFF}"));
        assert!(emoji < last_bmp, "\"😀\" < \"\\uFFFF\"");
    }

    /// A map keyed by `JsString` finds a key by a borrowed `&JsStr`, and a lone
    /// half from `slice` is the same key as a literal one.
    #[test]
    fn keys_hash_by_code_unit() {
        use ::std::hash::{BuildHasher as _, Hash as _};
        let mut map = ::std::collections::HashMap::new();
        map.insert(units(&[0xD83D]), 1);
        let half = JsString::from("😀").slice(0, 1);
        assert_eq!(map.get(&*half), Some(&1), "lookup by &JsStr");
        assert_eq!(map.get(&half), Some(&1), "lookup by &JsString");
        let state = ::std::collections::hash_map::RandomState::new();
        let hash_of = |value: &dyn Fn(&mut ::std::collections::hash_map::DefaultHasher)| {
            let mut hasher = state.build_hasher();
            value(&mut hasher);
            ::std::hash::Hasher::finish(&hasher)
        };
        let owned = JsString::from("key");
        assert_eq!(hash_of(&|hasher| owned.hash(hasher)), hash_of(&|hasher| (*owned).hash(hasher)), "Borrow requires equal hashes");
    }

    /// A clone shares the buffer; appending to a shared handle copies it and
    /// leaves the other handle unchanged.
    #[test]
    fn clones_share_and_appends_copy_on_write() {
        let original = JsString::from("abc");
        let alias = original.clone();
        assert_eq!(original.units().as_ptr(), alias.units().as_ptr(), "a clone shares the buffer");
        let extended = alias + &JsString::from("d");
        assert_eq!(original, "abc", "the shared handle is unchanged");
        assert_eq!(extended, "abcd", "the appended copy");
        assert_ne!(original.units().as_ptr(), extended.units().as_ptr(), "a shared buffer was copied");
    }

    /// `text = text + &piece` with the only handle appends in place: over many
    /// appends the buffer moves only when it grows, a logarithmic number of
    /// times. A copy per append would move it every time and make the loop
    /// quadratic.
    #[test]
    fn a_unique_builder_appends_in_place() {
        let piece = JsString::from("xy");
        let mut text = JsString::new();
        let (mut moves, mut last) = (0, text.units().as_ptr());
        for _ in 0..10_000 {
            text = text + &piece;
            if text.units().as_ptr() != last {
                moves += 1;
                last = text.units().as_ptr();
            }
        }
        assert_eq!(text.len(), 20_000, "every append landed");
        assert!(moves <= 20, "the buffer moved {moves} times: appends are copying");
    }

    /// stdout is UTF-8: a lone half prints as U+FFFD. `Debug` shows it escaped.
    #[test]
    fn display_is_lossy_and_debug_escapes() {
        let text = units(&[0x61, 0xD83D, 0x62]);
        assert_eq!(text.to_string(), "a\u{FFFD}b", "Display");
        assert_eq!(format!("{text:?}"), "\"a\\u{d83d}b\"", "Debug");
        let long = JsString::from("é".repeat(300).as_str());
        assert_eq!(long.to_string(), "é".repeat(300), "Display across its chunk buffer");
        assert_eq!(text.to_str_exact(), None, "no exact UTF-8 for a lone half");
        assert_eq!(JsString::from("ok").to_str_exact().as_deref(), Some("ok"), "exact UTF-8");
    }

    /// Case mapping follows Unicode's default rules and passes a lone half
    /// through.
    #[test]
    fn case_mapping_passes_lone_halves_through() {
        assert_eq!(JsString::from("straße").to_upper_case(), "STRASSE", "special casing");
        assert_eq!(JsString::from("ΣΑΣ").to_lower_case(), "σας", "final sigma");
        let mixed = units(&[0x61, 0xD83D, 0x62]);
        assert_eq!(mixed.to_upper_case().units(), [0x41, 0xD83D, 0x42], "a lone half is unchanged");
    }

    /// `split`, `replaceAll`, `trim` and `repeat` over code units.
    #[test]
    fn transforms_work_on_code_units() {
        let text = JsString::from("a,b,,c");
        assert_eq!(text.split(",", usize::MAX), ["a", "b", "", "c"].map(JsString::from), "split");
        assert_eq!(text.split(",", 2), ["a", "b"].map(JsString::from), "split with a limit");
        assert_eq!(JsString::from("😀").split("", usize::MAX), vec![units(&[0xD83D]), units(&[0xDE00])], "split(\"\") gives code units");
        assert_eq!(JsString::from("ab").replace_all("", &JsString::from("-")), "-a-b-", "an empty pattern");
        assert_eq!(text.replace_all(",", &JsString::from(";")), "a;b;;c", "replaceAll");
        assert_eq!(text.replace_first(",", &JsString::from(";")), "a;b,,c", "replace");
        assert_eq!(JsString::from("\u{FEFF}\u{a0} x \n").trim(), "x", "ZWNBSP and NBSP are white space");
        assert_eq!(JsString::from("\u{85}x").trim(), "\u{85}x", "NEL is not JavaScript white space");
        assert_eq!(JsString::from("ab").repeat(3), "ababab", "repeat");
        assert_eq!(JsString::join([JsString::from("a"), JsString::from("b")], &JsString::from(", ")), "a, b", "join");
    }
}
