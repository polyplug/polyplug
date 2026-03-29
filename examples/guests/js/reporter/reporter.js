import { setReporterImpl, polyplug_init } from './generated/guest/index';

function report(input) {
    const data = input.replace("TRANSFORMED:", "");
    const parts = data.split("|");
    if (parts.length >= 3) {
        return `Report: ${parts[0]} has value '${parts[1]}' with count ${parts[2]}`;
    }
    return "INVALID:format";
}

setReporterImpl(report);

export { polyplug_init };