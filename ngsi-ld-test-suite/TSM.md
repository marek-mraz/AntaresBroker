## Test Suite Management (tsm)

The tsm (Test Suite Management) tool is a command-line script designed to help selectively manage and execute test
cases from the NGSI-LD Test Suite — particularly useful when a Context Broker only implements a subset of the
NGSI-LD API.

The `tsm` script provides a set of commands to enable or disable Test Cases, Robot Test Suites or Collections
(Robot Test Suite Groups), visualise the current status of the different Test Cases, execute the selected Test Cases
and perform other related operations as described below.

The `tsm` script generates a pickle file named `tsm.pkl`, which stores the tuples list corresponding to each Test Case 
with the following information:

    (switch, running status, Test Case long name)

where the values and their meaning are the following:
* **switch**:
  * `ON`: the Test Case is on, which means that it will be executed by the script.
  * `OFF`: the Test Case is off, and therefore is not selected to be executed by the script.
  * `MISSING`: the Test Case is not anymore in the Test Suite Structure. An update operation should be run to update 
  the `tsm.pkl` with the current set of available Test Cases in the filesystem.
  * `NEW`: new Test Case discovered by the tsm after an update operation.
* **status**:
  * `PASSED`: the robot framework executes the Test Case with result PASS.
  * `FAILED`: the robot framework executes the Test Case with result FAIL.
  * `PENDING`: the Test Case is pending to be executed by the robot framework.
* **test case long name**: the Test Case long name set by Robot Framework based on the Robot Test Suite number and 
the Test Case name (e.g. NGSILD.032 02.032_02_01 Delete Unknown Subscription)

### Installation
The `tsm` script is integrated with argument auto-completion for bash, therefore it is needed the installation of 
the argcomplete python package on your system:

    pip install argcomplete

and to enable the completion after the installation, execute the following command:

    activate-global-python-argcomplete 

and then 

    eval "$(register-python-argcomplete tsm)"

Now, it is possible to autocomplete the commands and show possible options for executing the script. Also –help argument 
is available to obtain more information about the options.

### Execution

The tsm cases update command updates the `tsm.pkl` file with all the Robot Test Suite under the local path. If the 
pickle file does not exist, it is created. After the creation of this file, it is possible to execute the script 
to maintain and run the selected Test Cases from the pickle file. The list of commands is the following:

* **Test Cases (cases)**
  * Switch ON Test Cases
  
        tsm cases on [test_cases]
  
  * Switch OFF Test Cases

        tsm cases off [test_cases]
        tsm cases off "NGSILD.032 01.032_01_02 InvalidId"
  
  * List Test Cases based on the specific flag.
        
        tsm cases list [on, off, missing, new, passed, failed, pending, all]
        tsm cases list ./TP/NGSI-LD/CommonBehaviours
  
  * Run Test Cases that are enabled

        tsm cases run [on, off, missing, new, passed, failed, pending, [test_cases]]
        tsm cases run NGSILD.048\ 01.048_01_06\ Endpoint\ post\ /temporal/entities/
        tsm cases run pending
      
  * Update the pickle file with the current Test Cases

        tsm cases update
       
  * Clean Test Cases, remove the Test Cases that were marked as MISSING

        tsm cases clean

* **Robot Test Suites (suites)**
  * Switch ON Robot Test Suites

        tsm suites on [suites]
       
  * Switch OFF Robot Test Suites

        tsm suites off [suites]

* **Test Collections (collections)**
  * Switch ON Test Collections

        tsm collections on [collections]
        tsm collections on ./TP/NGSI-LD/CommonBehaviours

  * Switch OFF Test Collections

        tsm collections off [collections]