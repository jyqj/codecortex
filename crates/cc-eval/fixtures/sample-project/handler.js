const { formatName, Logger } = require('./utils');
const logger = new Logger();

function processUser(user) {
    const name = formatName(user.first, user.last);
    logger.log('Processing ' + name);
    return { formatted: name };
}

module.exports = { processUser };
