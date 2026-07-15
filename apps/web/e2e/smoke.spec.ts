import { expect, test } from '@playwright/test';

test('shows read-only banner and empty state when no snapshot exists', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('Read-only inventory snapshot')).toBeVisible();
  await expect(page.getByText('No snapshot published yet')).toBeVisible();
});
