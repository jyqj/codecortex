const { Logger } = require('./utils');
const logger = new Logger();

function authMiddleware(req, res, next) {
    if (!req.headers.authorization) {
        logger.log('Auth failed');
        return res.status(401).json({ error: 'Unauthorized' });
    }
    next();
}

function loggingMiddleware(req, res, next) {
    logger.log(req.method + ' ' + req.url);
    next();
}

module.exports = { authMiddleware, loggingMiddleware };
