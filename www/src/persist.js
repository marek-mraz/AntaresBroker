// Persisted UI preferences. localStorage is not this app's own storage: a
// value can be left there by anything else on the origin, and a browser with
// site data blocked throws on the accessor itself rather than answering
// empty. Both are read as "no preference stored" — a board that will not
// render because one key holds junk is worse than a board with defaults.

export function load(key, fallback = null) {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) ?? fallback;
  } catch {
    return fallback;
  }
}

export function save(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A full or blocked store costs the preference, never the page.
  }
}
