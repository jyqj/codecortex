const express = require('express');
const { processUser } = require('./handler');

const app = express();

app.get('/users/:id', function getUserRoute(req, res) {
    const user = processUser({ first: 'John', last: 'Doe' });
    res.json(user);
});

app.post('/users', function createUserRoute(req, res) {
    res.json({ created: true });
});

app.delete('/users/:id', function deleteUserRoute(req, res) {
    res.json({ deleted: true });
});

module.exports = app;
