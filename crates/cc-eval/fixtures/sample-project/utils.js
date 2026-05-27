function formatName(first, last) {
    return first + " " + last;
}

function validateEmail(email) {
    return email.indexOf("@") >= 0;
}

class Logger {
    log(msg) {
        console.log("[LOG] " + msg);
    }
}

module.exports = { formatName, validateEmail, Logger };
