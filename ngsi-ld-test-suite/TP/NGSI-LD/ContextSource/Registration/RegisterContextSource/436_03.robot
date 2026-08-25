*** Settings ***
Documentation       Verify the 4.3.6.3 structural and overlap rules for proxied registrations.
...
...                 Clause 4.3.6.3: "An exclusive registration shall always relate to
...                 specific Attributes found on a single Entity. Thus, the registration
...                 shall define both: an entity id (i.e. an id pattern or Entity type
...                 defining a group of entities is not supported for exclusive
...                 registrations) [and] Attributes." And: "Once an exclusive Context
...                 Source Registration has been created, no further exclusive or
...                 redirect Context Source Registrations can be created for that same
...                 combination of Entity ID and Attributes." Redirect overlap stays
...                 legal: "operations are distributed to all registered Context
...                 Sources." Conflicts surface as 409 (5.9.2 Conflict; Table 6.3.2-1
...                 carries no Conflict type, AlreadyExists is its 409 mapping).
...
...                 Antares extension TP — no official TP asserts the negative side of
...                 these rules; several official _exc TPs even violated them (see
...                 testsuite-doubts.md 2026-08-10).

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${scoped_fixture}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld
${bare_fixture}         csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld
${pattern_fixture}      csourceRegistrations/context-source-registration-vehicle-speed-with-batch-ops.jsonld


*** Test Cases ***
436_03_01 Exclusive Registration Without Attributes Is Rejected
    [Documentation]    4.3.6.3: the registration shall define Attributes — an exclusive
    ...    registration naming only an entity id is invalid content (400 BadRequestData).
    [Tags]    csr-create    4_3_6_3    5_9_2    since_v1.9.1
    ${registration_id}=    Generate Random CSR Id
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${bare_fixture}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

436_03_02 Exclusive Registration With A Type-Only Entity Group Is Rejected
    [Documentation]    4.3.6.3: an Entity type defining a group of entities is not
    ...    supported for exclusive registrations (400 BadRequestData).
    [Tags]    csr-create    4_3_6_3    5_9_2    since_v1.9.1
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${pattern_fixture}
    ...    mode=exclusive
    ${payload}=    Delete Object From JSON    ${payload}    $.information[0].entities[0].idPattern
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

436_03_03 Exclusive Registration With An Id Pattern Is Rejected
    [Documentation]    4.3.6.3: an id pattern is not supported for exclusive
    ...    registrations (400 BadRequestData).
    [Tags]    csr-create    4_3_6_3    5_9_2    since_v1.9.1
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${pattern_fixture}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

436_03_04 Proxied Registration Overlapping An Exclusive One Is Rejected
    [Documentation]    4.3.6.3: once an exclusive registration exists, no further
    ...    exclusive or redirect registration can be created for the same combination
    ...    of Entity ID and Attributes (409).
    [Tags]    csr-create    4_3_6_3    5_9_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${first_id}=    Generate Random CSR Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${first_id}
    ...    ${scoped_fixture}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${second_id}=    Generate Random CSR Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${second_id}
    ...    ${scoped_fixture}
    ...    entity_id=${entity_id}
    ...    mode=exclusive
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    409    ${response.status_code}
    ${payload}=    Prepare Context Source Registration From File
    ...    ${second_id}
    ...    ${scoped_fixture}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    409    ${response.status_code}
    [Teardown]    Delete Context Source Registration    ${first_id}

436_03_05 Redirect Registrations May Overlap Each Other
    [Documentation]    4.3.6.3: "in the case that multiple overlapping redirect
    ...    registrations are defined, operations are distributed to all registered
    ...    Context Sources" — the second overlapping redirect is accepted.
    [Tags]    csr-create    4_3_6_3    5_9_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${first_id}=    Generate Random CSR Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${first_id}
    ...    ${scoped_fixture}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${second_id}=    Generate Random CSR Id
    ${payload}=    Prepare Context Source Registration From File
    ...    ${second_id}
    ...    ${scoped_fixture}
    ...    entity_id=${entity_id}
    ...    mode=redirect
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    201    ${response.status_code}
    [Teardown]    Run Keywords    Delete Context Source Registration    ${first_id}
    ...    AND    Delete Context Source Registration    ${second_id}
