#include <string>

namespace banking {

// A bank account with a balance.
class Account {
public:
    explicit Account(double balance) : balance_(balance) {}

    // Returns true if the account has at least `amount`.
    bool has_funds(double amount) const {
        return balance_ >= amount;
    }

    // Deposit money into the account.
    void deposit(double amount) {
        balance_ += amount;
    }

    // Withdraw money if funds are sufficient. Calls has_funds().
    bool withdraw(double amount) {
        if (!has_funds(amount)) {
            return false;
        }
        balance_ -= amount;
        return true;
    }

    double balance() const {
        return balance_;
    }

private:
    double balance_;
};

// Transfer money between two accounts. Calls withdraw() and deposit().
bool transfer(Account &from, Account &to, double amount) {
    if (!from.withdraw(amount)) {
        return false;
    }
    to.deposit(amount);
    return true;
}

}  // namespace banking
