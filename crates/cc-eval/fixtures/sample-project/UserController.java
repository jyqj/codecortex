package com.example.api.controller;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.web.bind.annotation.*;
import java.util.List;
import java.util.Map;
import java.util.HashMap;

/**
 * Base controller providing shared helper methods.
 */
abstract class BaseController {

    protected Map<String, Object> ok(Object data) {
        Map<String, Object> result = new HashMap<>();
        result.put("status", "ok");
        result.put("data", data);
        return result;
    }

    protected Map<String, Object> error(String message) {
        Map<String, Object> result = new HashMap<>();
        result.put("status", "error");
        result.put("message", message);
        return result;
    }
}

/**
 * REST controller for user management.
 */
@RestController
@RequestMapping("/api/users")
public class UserController extends BaseController {

    @Autowired
    private UserService userService;

    @GetMapping("")
    public List<Map<String, Object>> listUsers() {
        return userService.findAll();
    }

    @GetMapping("/{id}")
    public Map<String, Object> getUser(@PathVariable Long id) {
        Map<String, Object> user = userService.findById(id);
        if (user == null) {
            return error("User not found");
        }
        return ok(user);
    }

    @PostMapping("")
    public Map<String, Object> createUser(@RequestBody Map<String, String> body) {
        String name = body.get("name");
        if (name == null || name.isEmpty()) {
            return error("Name is required");
        }
        Map<String, Object> user = userService.create(name);
        return ok(user);
    }

    @DeleteMapping("/{id}")
    public Map<String, Object> deleteUser(@PathVariable Long id) {
        boolean deleted = userService.delete(id);
        if (!deleted) {
            return error("User not found");
        }
        return ok(Map.of("deleted", true));
    }

    /**
     * Private helper to validate user ID range.
     */
    private boolean isValidId(Long id) {
        return id != null && id > 0;
    }

    /**
     * Build a summary string for a user record.
     */
    private String buildUserSummary(Map<String, Object> user) {
        return String.format("User(id=%s, name=%s)",
            user.get("id"), user.get("name"));
    }
}
