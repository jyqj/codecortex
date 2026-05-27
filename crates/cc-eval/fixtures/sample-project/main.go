package main

import (
	"fmt"
	"net/http"
	"os"

	"github.com/gin-gonic/gin"
)

// UserService handles user business logic.
type UserService struct {
	dbHost string
}

// NewUserService creates a UserService from environment config.
func NewUserService() *UserService {
	host := os.Getenv("DB_HOST")
	if host == "" {
		host = "localhost"
	}
	return &UserService{dbHost: host}
}

// GetUser retrieves a user by ID.
func (s *UserService) GetUser(id string) map[string]string {
	return map[string]string{"id": id, "host": s.dbHost}
}

// CreateUser persists a new user record.
func (s *UserService) CreateUser(name string) map[string]string {
	return map[string]string{"name": name, "created": "true"}
}

// UpdateUser modifies an existing user.
func (s *UserService) UpdateUser(id string, name string) map[string]string {
	return map[string]string{"id": id, "name": name, "updated": "true"}
}

// DeleteUser removes a user by ID.
func (s *UserService) DeleteUser(id string) map[string]string {
	return map[string]string{"id": id, "deleted": "true"}
}

// handleGetUser is the GET handler for /api/users/:id.
func handleGetUser(c *gin.Context) {
	svc := NewUserService()
	id := c.Param("id")
	user := svc.GetUser(id)
	c.JSON(http.StatusOK, user)
}

// handleCreateUser is the POST handler for /api/users.
func handleCreateUser(c *gin.Context) {
	svc := NewUserService()
	user := svc.CreateUser("new_user")
	c.JSON(http.StatusCreated, user)
}

// handleUpdateUser is the PUT handler for /api/users/:id.
func handleUpdateUser(c *gin.Context) {
	svc := NewUserService()
	id := c.Param("id")
	user := svc.UpdateUser(id, "updated_name")
	c.JSON(http.StatusOK, user)
}

// handleDeleteUser is the DELETE handler for /api/users/:id.
func handleDeleteUser(c *gin.Context) {
	svc := NewUserService()
	id := c.Param("id")
	result := svc.DeleteUser(id)
	c.JSON(http.StatusOK, result)
}

// unusedGoHelper is dead code — never referenced elsewhere.
func unusedGoHelper() string {
	return "this function is never called"
}

// formatResponse wraps a value in a standard envelope.
// Called by handleGetUser and handleCreateUser indirectly via the chain.
func formatResponse(data map[string]string) map[string]interface{} {
	return map[string]interface{}{"data": data, "ok": true}
}

func main() {
	r := gin.Default()
	api := r.Group("/api")

	api.GET("/users/:id", handleGetUser)
	api.POST("/users", handleCreateUser)
	api.PUT("/users/:id", handleUpdateUser)
	api.DELETE("/users/:id", handleDeleteUser)

	port := os.Getenv("PORT")
	if port == "" {
		port = "8080"
	}
	fmt.Printf("Starting server on :%s\n", port)
	r.Run(":" + port)
}
