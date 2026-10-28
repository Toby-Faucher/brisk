/** Copy text: the clipboard API, else select the text in `field` and try the old copy command, else leave it selected. */
export async function copyText(text: string, field?: HTMLInputElement | null): Promise<'copied' | 'selected'> {
  try {
    await navigator.clipboard.writeText(text);
    return 'copied';
  } catch {}
  field?.focus();
  field?.select();
  try {
    if (field && document.execCommand('copy')) return 'copied';
  } catch {}
  return 'selected';
}
