/**
 * @file reload_phase.js
 * @description ReloadPhase class for hot-reload notifications.
 *
 * This module provides the ReloadPhase class used by the hot-reload
 * notification system to communicate reload state changes to the host.
 *
 * @module polyplug/reload_phase
 */

/**
 * Notification phase for hot-reload operations.
 *
 * Used by the reload callback to notify the host about reload progress.
 * Mirrors the C ABI callback parameters for hot-reload notifications.
 */
export class ReloadPhase {
    /** Phase type constant: Before interface swap, host should cleanup instances */
    static TYPE_PREPARING = 0;
    /** Phase type constant: After interface swap, instances can be re-resolved */
    static TYPE_RELOADED = 1;
    /** Phase type constant: Reload aborted after max retries */
    static TYPE_FAILED = 2;

    /**
     * Create a new ReloadPhase instance.
     *
     * Mirrors the ABI `ReloadPhase` struct exactly — there is no retry_count
     * field in the ABI.
     * @param {number} type - Phase type (TYPE_PREPARING, TYPE_RELOADED, or TYPE_FAILED)
     * @param {bigint} bundleId - FNV-1a 64-bit hash of the bundle name
     * @param {string} bundleName - Human-readable bundle name
     * @param {string} [reason=""] - Error reason (only for Failed phase)
     */
    constructor(type, bundleId, bundleName, reason = "") {
        /** @type {number} Phase type */
        this.type = type;
        /** @type {bigint} Bundle ID */
        this.bundleId = bundleId;
        /** @type {string} Bundle name */
        this.bundleName = bundleName;
        /** @type {string} Error reason */
        this.reason = reason;
    }

    /**
     * Check if this is a Preparing phase.
     * @returns {boolean}
     */
    isPreparing() {
        return this.type === ReloadPhase.TYPE_PREPARING;
    }

    /**
     * Check if this is a Reloaded phase.
     * @returns {boolean}
     */
    isReloaded() {
        return this.type === ReloadPhase.TYPE_RELOADED;
    }

    /**
     * Check if this is a Failed phase.
     * @returns {boolean}
     */
    isFailed() {
        return this.type === ReloadPhase.TYPE_FAILED;
    }

    /**
     * Get string representation.
     * @returns {string}
     */
    toString() {
        const typeNames = {
            [ReloadPhase.TYPE_PREPARING]: "Preparing",
            [ReloadPhase.TYPE_RELOADED]: "Reloaded",
            [ReloadPhase.TYPE_FAILED]: "Failed",
        };
        const typeName = typeNames[this.type] || `Unknown(${this.type})`;
        return `ReloadPhase(type=${typeName}, bundleId=${this.bundleId}, bundleName=${JSON.stringify(this.bundleName)}, reason=${JSON.stringify(this.reason)})`;
    }
}