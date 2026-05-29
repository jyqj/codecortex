#include "geometry.h"

/* Multiply two integers. */
int multiply(int a, int b) {
    return a * b;
}

/* Add two integers. */
int add(int a, int b) {
    return a + b;
}

/* Compute the area of a rectangle by multiplying width and height. */
int compute_area(Rectangle rect) {
    return multiply(rect.width, rect.height);
}

/* Compute the perimeter: 2 * (width + height). */
int rectangle_perimeter(Rectangle rect) {
    int half = add(rect.width, rect.height);
    return multiply(2, half);
}
