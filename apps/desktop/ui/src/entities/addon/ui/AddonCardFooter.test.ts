/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';

import AddonCardFooterTestHost from './AddonCardFooter.test-host.svelte';

describe('AddonCardFooter', () => {
  let target: HTMLDivElement;
  let component: object | undefined;

  function render(
    props: {
      showLeading?: boolean;
      showActions?: boolean;
      class?: string;
    } = {},
  ): void {
    component = mount(AddonCardFooterTestHost, {
      target,
      props,
    });
    flushSync();
  }

  beforeEach(() => {
    target = document.createElement('div');
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) {
      await unmount(component);
      component = undefined;
    }
    target.remove();
  });

  it('renders with data-slot and standard footer styling', () => {
    render();

    const footer = target.querySelector('[data-slot="addon-card-footer"]');
    expect(footer).not.toBeNull();
    expect(footer?.className).toContain('mt-auto');
    expect(footer?.className).not.toContain('border-t');
  });

  it('renders leading attribution content and action buttons', () => {
    render({ showLeading: true, showActions: true });

    expect(target.querySelector('[data-testid="leading-content"]')?.textContent).toBe(
      'Attribution Info',
    );
    expect(target.querySelector('[data-testid="action-btn"]')?.textContent).toBe('Action');
  });

  it('renders actions aligned to the end with sm:ms-auto when leading is omitted', () => {
    render({ showLeading: false, showActions: true });

    expect(target.querySelector('[data-testid="leading-content"]')).toBeNull();
    const actionBtn = target.querySelector('[data-testid="action-btn"]');
    expect(actionBtn).not.toBeNull();

    const actionsContainer = actionBtn?.parentElement;
    expect(actionsContainer?.className).toContain('sm:ms-auto');
    expect(actionsContainer?.className).toContain('sm:justify-end');
  });
});
