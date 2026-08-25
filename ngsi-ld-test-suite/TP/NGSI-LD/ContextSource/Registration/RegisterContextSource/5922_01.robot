*** Settings ***
Documentation       Verify 5.9.2.4 Register Context Source edges the official
...                 TPs skip.
...
...                 5.9.2.4: an auxiliary registration whose operations are
...                 not retrieveOps/retrieveEntity/queryEntity (or a
...                 combination) is BadRequestData; an exclusive
...                 registration conflicts (409) with an existing Entity
...                 carrying any of its Attributes; a redirect registration
...                 conflicts (409) with any existing matching Entity.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
5922_01_01 Auxiliary Registration With Write Ops Is BadRequestData
    [Documentation]    5.9.2.4: mode=auxiliary with a write operation → 400.
    [Tags]    csr-create    5_9_2    since_v1.9.1
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Evaluate
    ...    {"id": "${registration_id}", "type": "ContextSourceRegistration", "mode": "auxiliary", "operations": ["updateEntity"], "information": [{"entities": [{"type": "Building"}]}], "endpoint": "http://source.example.com", "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}

5922_01_02 Redirect Registration Over An Existing Entity Is Conflict
    [Documentation]    5.9.2.4: "If an existing Entity already matches the
    ...    Context Source Registration, an error of type Conflict shall be
    ...    raised" (mode=redirect).
    [Tags]    csr-create    5_9_2    since_v1.9.1
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Evaluate
    ...    {"id": "${registration_id}", "type": "ContextSourceRegistration", "mode": "redirect", "operations": ["retrieveOps"], "information": [{"entities": [{"type": "Building", "id": "${entity_id}"}]}], "endpoint": "http://source.example.com", "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    409    ${response.status_code}
    Should Contain    ${response.text}    Conflict
    [Teardown]    Delete Entity    ${entity_id}

5922_01_03 Exclusive Registration Over An Existing Attribute Is Conflict
    [Documentation]    5.9.2.4: "If an Entity already exists for the
    ...    supplied Entity ID (URI) and the existing Entity contains any of
    ...    the Attributes defined in the registration, an error of type
    ...    Conflict shall be raised" (mode=exclusive).
    [Tags]    csr-create    5_9_2    since_v1.9.1
    ${entity_id}=    Generate Random Building Entity Id
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Evaluate
    ...    {"id": "${registration_id}", "type": "ContextSourceRegistration", "mode": "exclusive", "operations": ["retrieveOps"], "information": [{"entities": [{"type": "Building", "id": "${entity_id}"}], "propertyNames": ["subCategory"]}], "endpoint": "http://source.example.com", "@context": ["${ngsild_test_suite_context}"]}
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    409    ${response.status_code}
    Should Contain    ${response.text}    Conflict
    [Teardown]    Delete Entity    ${entity_id}
