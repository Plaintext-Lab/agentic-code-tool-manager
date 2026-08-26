import type { TranslationKey } from '$lib/i18n';
import type { InventoryActionBlockedReason, InventoryClient } from '$lib/types';

export const inventoryClientLabels: Record<InventoryClient, string> = {
	claude: 'Claude',
	codex: 'Codex',
	cursor: 'Cursor'
};

export const blockedReasonLabels: Record<InventoryActionBlockedReason, TranslationKey> = {
	alreadyEnabled: 'inventory.actionAlreadyEnabled',
	alreadyDisabled: 'inventory.actionAlreadyDisabled',
	stateUnavailable: 'inventory.actionStateUnavailable',
	managedSource: 'inventory.actionManagedSource',
	administratorSource: 'inventory.actionAdministratorSource',
	policyControlled: 'inventory.actionPolicyControlled',
	pluginOwnedSource: 'inventory.actionPluginOwnedSource',
	malformedSource: 'inventory.actionMalformedSource',
	brokenSymlink: 'inventory.actionBrokenSymlink',
	unsupportedByClient: 'inventory.actionUnsupportedByClient'
};
