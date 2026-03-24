import { setReporterImpl, polyplug_init } from './generated/guest/index';

function report(input) {
    return input;
}

setReporterImpl(report);

export { polyplug_init };