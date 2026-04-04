export const POLYPLUG_ABI_VERSION: number = 1;

export function fnv1a_64(data: &[u8]): bigint {}

export function contract_id(name: &str, major: number): bigint {}

export function bundle_id(name: &str): bigint {}

export function host_contract_id(name: &str, major: number): bigint {}

export function plugin_contract_id(name: &str, major: number): bigint {}

