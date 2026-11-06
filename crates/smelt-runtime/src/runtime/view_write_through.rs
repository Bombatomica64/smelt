// Write-through of erased views onto their live class instances.

// @smelt:item smelt_object_write_through when unknown & write_through
/// Forward a property write on an erased class view to the instance it stands for.
fn smelt_object_write_through(map: &SmeltObject, key: &str, value: &SmeltUnknown) { if let Some(SmeltUnknown::Function(setter)) = map.get("__smelt_method:__smelt_set_field") { let _ = setter(vec![SmeltUnknown::String(key.into()), value.clone()]); } }
// @smelt:item-end

// @smelt:item smelt_live_member when reads_this & unknown
/// Read `key` off an erased value, asking a class view's live callable-field getter first.
#[allow(dead_code)]
fn smelt_live_member(value: &SmeltUnknown, key: &str) -> SmeltUnknown { if let SmeltUnknown::Object(map) = value { if let Some(SmeltUnknown::Function(getter)) = map.get("__smelt_method:__smelt_get_callable_field") { if let Ok(live) = getter(vec![SmeltUnknown::String(key.into())]) { if !matches!(live, SmeltUnknown::Undefined) { return live; } } } } smelt_get_unknown_field(value, key) }
// @smelt:item-end
