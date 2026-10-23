// The tab is being sent elsewhere on purpose (sign-out, another user, a new
// sign-in): forms with unsaved changes must not hold it back.
let leaving = false;

/** Called right before the session reloads or replaces the page. */
export function leaveOnPurpose(): void {
  leaving = true;
}

/** Whether the page is being left on purpose (see {@link leaveOnPurpose}). */
export function leavingOnPurpose(): boolean {
  return leaving;
}

/** Clears {@link leaveOnPurpose} (tests: jsdom never leaves the page). */
export function stayOnPurpose(): void {
  leaving = false;
}
