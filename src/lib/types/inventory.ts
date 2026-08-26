export type InventoryClient = 'claude' | 'codex' | 'cursor';
export type InventoryItemType = 'skill' | 'mcp' | 'hook';
export type InventoryScope = 'user' | 'project' | 'admin' | 'legacy';
export type InventorySourceKind =
	| 'userConfig'
	| 'projectConfig'
	| 'localConfig'
	| 'managedConfig'
	| 'userSkills'
	| 'projectSkills'
	| 'adminSkills'
	| 'legacySkills'
	| 'pluginConfig'
	| 'pluginSkills';
export type InventoryTrustState = 'notApplicable' | 'unknown' | 'trusted' | 'untrusted';
export type InventoryActionBlockedReason =
	| 'alreadyEnabled'
	| 'alreadyDisabled'
	| 'stateUnavailable'
	| 'managedSource'
	| 'administratorSource'
	| 'policyControlled'
	| 'pluginOwnedSource'
	| 'malformedSource'
	| 'brokenSymlink'
	| 'unsupportedByClient';
export type InventoryReloadGuidance = 'notRequired' | 'restartClient';

export interface InventoryActionAvailability {
	available: boolean;
	blockedReason: InventoryActionBlockedReason | null;
}

export interface InventoryActionCapabilities {
	enable: InventoryActionAvailability;
	disable: InventoryActionAvailability;
	confirmationRequired: boolean;
	reloadGuidance: InventoryReloadGuidance;
	sourceRevision: string | null;
}

export interface AdapterCapabilities {
	client: InventoryClient;
	skills: boolean;
	mcps: boolean;
	hooks: boolean;
}

export interface InventoryRecord {
	id: string;
	client: InventoryClient;
	itemType: InventoryItemType;
	name: string;
	scope: InventoryScope;
	sourceKind: InventorySourceKind;
	sourcePath: string;
	projectPath: string | null;
	originalPath: string;
	resolvedPath: string | null;
	isSymlink: boolean;
	enabled: boolean | null;
	trustState: InventoryTrustState;
	isEffective: boolean | null;
	sourcePriority: number;
	protectedFields: string[];
	detail: string | null;
	actionCapabilities: InventoryActionCapabilities;
}

export interface InventoryWarning {
	client: InventoryClient | null;
	sourcePath: string;
	message: string;
}

export interface InventorySnapshot {
	records: InventoryRecord[];
	warnings: InventoryWarning[];
	capabilities: AdapterCapabilities[];
	scannedProjectCount: number;
}
