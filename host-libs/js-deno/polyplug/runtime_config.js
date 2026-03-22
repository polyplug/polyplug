/**
 * @file runtime_config.js
 * @description Runtime configuration options for hot-reload behavior.
 *
 * This module provides the RuntimeConfig class for configuring
 * hot-reload behavior and other runtime settings.
 *
 * @module polyplug/runtime_config
 */

/**
 * Configuration options for the Runtime.
 *
 * This class contains configurable parameters for hot-reload behavior
 * and other runtime settings. It is designed to be extensible for future options.
 */
export class RuntimeConfig {
    /**
     * Create a new RuntimeConfig instance.
     * @param {Object} [options={}] - Configuration options
     * @param {number} [options.hotReloadMaxRetries=3] - Maximum retry attempts for hot-reload.
     *   Set to 0 for infinite retries when hotReloadAbortOnMaxRetries is false.
     * @param {number} [options.hotReloadRetryIntervalMs=1000] - Interval between retry attempts in milliseconds.
     * @param {boolean} [options.hotReloadAbortOnMaxRetries=true] - Whether to abort after max retries.
     *   If true: abort and fire Failed notification.
     *   If false: keep retrying forever.
     */
    constructor({
        hotReloadMaxRetries = 3,
        hotReloadRetryIntervalMs = 1000,
        hotReloadAbortOnMaxRetries = true,
    } = {}) {
        /** @type {number} Maximum retry attempts for hot-reload */
        this.hotReloadMaxRetries = hotReloadMaxRetries;
        /** @type {number} Interval between retry attempts in milliseconds */
        this.hotReloadRetryIntervalMs = hotReloadRetryIntervalMs;
        /** @type {boolean} Whether to abort after max retries */
        this.hotReloadAbortOnMaxRetries = hotReloadAbortOnMaxRetries;
    }

    /**
     * Create a RuntimeConfig with default values.
     * @returns {RuntimeConfig}
     */
    static default() {
        return new RuntimeConfig();
    }

    /**
     * Create a RuntimeConfig for infinite retries.
     * @param {number} [retryIntervalMs=1000] - Interval between retries in milliseconds
     * @returns {RuntimeConfig}
     */
    static infiniteRetries(retryIntervalMs = 1000) {
        return new RuntimeConfig({
            hotReloadMaxRetries: 0,
            hotReloadRetryIntervalMs: retryIntervalMs,
            hotReloadAbortOnMaxRetries: false,
        });
    }

    /**
     * Create a RuntimeConfig with custom retry count.
     * @param {number} maxRetries - Maximum number of retries
     * @param {number} [retryIntervalMs=1000] - Interval between retries in milliseconds
     * @returns {RuntimeConfig}
     */
    static withRetries(maxRetries, retryIntervalMs = 1000) {
        return new RuntimeConfig({
            hotReloadMaxRetries: maxRetries,
            hotReloadRetryIntervalMs: retryIntervalMs,
            hotReloadAbortOnMaxRetries: true,
        });
    }
}