package main

import "math/rand"

var shapes = []string{"circle", "square", "triangle", "star", "hexagon"}

func RandomShape() string {
	return shapes[rand.Intn(len(shapes))]
}
