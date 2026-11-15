// A write or delete through an `as any` view of a declared record, keyed by a
// run-time string (date-fns `setDefaultOptions`, which copies option keys onto
// its defaults object one by one).
interface Options {
  weekStartsOn?: number;
  firstWeekContainsDate?: number;
  locale?: string;
}

class Settings {
  zone = "UTC";
  hour12 = false;
}

function assign(target: Options, source: Record<string, unknown>): Options {
  for (const key in source) {
    if (source[key] === undefined) {
      delete (target as any)[key];
    } else {
      (target as any)[key] = source[key];
    }
  }
  return target;
}

const options: Options = { weekStartsOn: 0, locale: "en" };
assign(options, { weekStartsOn: 1, firstWeekContainsDate: 4 });
console.log(options.weekStartsOn, options.firstWeekContainsDate, options.locale);
assign(options, { locale: undefined, weekStartsOn: 6 });
console.log(options.weekStartsOn, options.locale, "locale" in options, Object.keys(options).join(","));

const settings = new Settings();
const key: string = ["zone", "hour12"][0];
(settings as unknown as Record<string, unknown>)[key] = "Europe/Rome";
(settings as any)["hour" + "12"] = true;
console.log(settings.zone, settings.hour12);
// A key the record does not declare is no field of it.
(settings as any)[String("unknownKey")] = 1;
console.log(settings.zone, settings.hour12);
