# 3rd Report - ESOF

## Index

1. [Introduction](#introduction)
2. [Logical View](#logical-view)
3. [Implementation View](#implementation-view)
4. [Process View](#process-view)
5. [Deployment View](#deployment-view)
6. [Critical Analysis](#critical-analysis)

## Introduction

This report aims to explain some apects relating the software architecture of the Rust project, according to the [4+1 Architecture view model].

**4+1** is a **view model** designed for describing the architecture of software-intensive systems.

We will be presenting four components regarding the [4+1 Architecture view model] which are:

1. [Logical View](#logical-view)
2. [Implementation View](#implementation-view)
3. [Process View](#process-view)
4. [Deployment View](#deployment-view)

Each one of this view has one or more type of UML diagrams that can be used to represent itself.

The **views** are used to describe the system from the viewpoint of different stakeholders,such as developers and project managers.

![alt tag](https://raw.githubusercontent.com/martapips/rust/master/ESOF-docs/res/4plus1.gif)

[4+1 Architecture view model]:https://en.wikipedia.org/wiki/4%2B1_architectural_view_model

## Logical View

This kind of view concerns the functionality that the language provides to the end-user. The UML diagrams that can be used to represent the logical view are: [Sequece Diagram], [Communication Diagram] and [Class Diagram].

A **Sequence Diagram** shows how processes operate with one another and in what order.

It also shows object interactions arranged in time sequence.

A **Communication Diagram** represent a combination of information taken from **Class**, **Sequence** and                **Use Case Diagrams** describing both the static structure and dynamic behavior of the system.

A **Class Diagram** describes the structure of a system by showing the system´s **classes**,their atributes,methods and the relationships among objects.


[Sequece Diagram]:https://en.wikipedia.org/wiki/Sequence_diagram
[Communication Diagram]:https://en.wikipedia.org/wiki/Communication_diagram
[Class Diagram]:https://en.wikipedia.org/wiki/Class_diagram

![alt tag](https://github.com/martapips/rust/blob/master/ESOF-docs/res/packageDiagram.jpg?raw=true)

## Implementation View

This view shows the program from a different prespective than the previous one. From the prespective of the programmer. It is used the [Component Diagram] to show the programmer the program's different components.

**Component Diagrams** are used to model physical aspects of a system.

Physical aspects are the elements like executables,libraries,files and documents which reside in a node.

The purpose of the component diagram can be summarized as :

* Visualize the components of the system

* Contruct executables by using reverse engineering

* Describe the organization and relationships of the components

[Component Diagram]:http://www.tutorialspoint.com/uml/uml_component_diagram.htm

![alt tag](https://github.com/martapips/rust/blob/master/ESOF-docs/res/componentDiagram.jpg?raw=true)

## Process View

This view is foccused on the runtime behaviour of the program. It is in this view that the programs processes are explained as well as the way they use to communicate between them. [Activity Diagrams] are the ones used to describe this view.

**Activity diagrams** are graphical representations of **workflows** of stepwise activities and actions.

In UML activity diagrams are intended to model both computational and organizational processes.

Activity diagrams are constructed from a limited number of shapes,connected with arrows.

![alt tag](https://github.com/martapips/rust/blob/master/ESOF-docs/res/activity.jpg?raw=true)

[Activity Diagrams]:https://en.wikipedia.org/wiki/Activity_diagram

## Deployment View

This view shows the program in the system engineer's prespective.  To represent this view we use [Deployment Diagrams].
It is concerned with the topology of software components on the physical layer, as well as the physical connections between these components.

A **Deployment Diagram** models the physical deployment of artifacts on nodes.

![alt tag](https://github.com/martapips/rust/blob/master/ESOF-docs/res/deploy.jpg?raw=true)

[Deployment Diagrams]:https://en.wikipedia.org/wiki/Deployment_diagram

## Critical Analysis

It's important to say that all the diagrams presented in this report were built from zero by us, which were based only in our compreension and understanding of how the project worked.

The **Implementation View** is expressed by a **Component Diagram**  that clearly shows how a file, that contains **Rust** code, is treated along the process and be ready to run.

The **Process View** is expressed by a **Activity Diagram** that shows how **Rust** code is analysed in the compiler and all the transformations it suffers doing that analysis.

The **Deployment View** is expressed by a **Deployment Diagram** that shows the interaction of the user of the project(that can be a programmer that uses rust language) with the device that in this case is a computer that processes the **Rust** code written by the programmer.

The **Logical View** is expressed by a **Sequence Diagram**  that shows how all the processes, that are executed in parallel, interact with each other to make the main process work and to make it show the final results to the user of the program.

In the end, we can say that the **Rust** Project is well organized and has a good interaction user-project but it lacks in some aspect like where to find all the information needed to construct some of this diagrams. We think that this is an important matter that the developers could work a bit more because it could make even easier for someone else to analise this project.
