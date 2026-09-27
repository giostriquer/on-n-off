/**
 * Focus handed on when an action takes away the control that holds it, as archiving, unarchiving or
 * removing an account unmounts the button the user pressed. Focus moves only while it is still where
 * the user left it, so an action that resolves late never pulls focus from wherever the user went.
 */

/** Focus is on the page itself: nothing, the body, or an element that has left the document. */
function onPage(element: Element | null): boolean {
  return !element || element === document.body || !element.isConnected;
}

/**
 * Notes the focused element before an action that may take it away. `unmoved()` answers afterwards
 * whether focus is still on it, or fell to the page because it was disabled or removed.
 */
export function holdFocus(): () => boolean {
  const held = document.activeElement;
  return () => document.activeElement === held || onPage(document.activeElement);
}
