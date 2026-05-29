#ifndef GEOMETRY_H
#define GEOMETRY_H

/* A simple 2D rectangle. */
typedef struct {
    int width;
    int height;
} Rectangle;

/* Multiply two integers. */
int multiply(int a, int b);

/* Add two integers. */
int add(int a, int b);

/* Compute the area of a rectangle. Calls multiply(). */
int compute_area(Rectangle rect);

/* Compute the perimeter of a rectangle. Calls add() and multiply(). */
int rectangle_perimeter(Rectangle rect);

#endif /* GEOMETRY_H */
