// @ts-check
// Shared axe-core pass for ui-smoke.mjs and axe-check.mjs: WCAG 2.x A/AA rules, and a page fails on any serious or
// critical violation that is not listed in EXCEPTIONS below.
import { AxeBuilder } from '@axe-core/playwright';

export const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];

/** Known exceptions: [rule id, text the node's HTML must contain, one-line justification]. Keep it short. */
export const EXCEPTIONS = [
  [
    'scrollable-region-focusable',
    'class="cm-list',
    'Bits Command listbox: the input keeps focus and scrolls it with arrow keys (aria-activedescendant)',
  ],
];

/** @param {string} rule @param {string} html */
const excepted = (rule, html) => EXCEPTIONS.some(([r, s]) => r === rule && html.includes(s));

/**
 * Runs axe on the page as it is now; returns one line per failing node (empty = pass).
 * @param {import('playwright-core').Page} page
 */
export async function axe(page) {
  const { violations } = await new AxeBuilder({ page }).withTags(TAGS).analyze();
  return violations
    .filter((v) => v.impact === 'serious' || v.impact === 'critical')
    .flatMap((v) =>
      v.nodes.map((n) => ({
        rule: v.id,
        impact: v.impact,
        html: n.html,
        target: n.target.join(' '),
        why: n.failureSummary?.split('\n').slice(0, 2).join(' '),
      })),
    )
    .filter((x) => !excepted(x.rule, x.html))
    .map((x) => `${x.impact} ${x.rule} @ ${x.target}: ${x.why}`);
}
