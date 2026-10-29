/** The text to show for a caught value: an Error's message, or the value itself (a rejection can be anything). */
export const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e));
