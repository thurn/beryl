let profile: string | null = null;

/** Called with the native app's profile before the UI reads settings. */
export function setProfile(name: string | null) {
  profile = name;
}

/** The localStorage key for this app instance. The default profile keeps its existing keys. */
export function key(name: string): string {
  return profile ? `beryl.profile:${profile}:${name}` : name;
}
