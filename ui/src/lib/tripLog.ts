export type TripTag = "ON" | "OFF" | "SYNC" | "INST" | "TRIP";

export type TripEntry = {
  at: string;
  tag: TripTag;
  text: string;
};

export function stampNow(): string {
  const date = new Date();
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function prependTrip(log: TripEntry[], tag: TripTag, text: string, limit = 7): TripEntry[] {
  return [{ at: stampNow(), tag, text }, ...log].slice(0, limit);
}

export function tripTagClass(tag: TripTag): string {
  if (tag === "TRIP") {
    return "bg-[var(--trip)] text-[#f7f1ea]";
  }
  if (tag === "ON") {
    return "bg-[var(--brass)] text-[var(--void)]";
  }
  return "bg-[var(--well)] text-[var(--mute)]";
}
