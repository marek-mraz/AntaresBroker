*** Settings ***
Documentation       Check that one can replace an existing entity and that scopes are replaced

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${entity_filename}                  building-minimal-with-one-scope.json
${entity_replacement_filename}      building-minimal-with-many-scopes.json
${expectation_filename}             building-minimal-with-two-scopes.json


*** Test Cases ***
054_03_01 Replace An Existing Entity Having Scopes
    [Documentation]    Check that one can replace an existing entity and that scopes are replaced
    [Tags]    e-replace    4_18    5_6_18    6_5_3_3    since_v1.6.1
    ${response}=    Replace Entity
    ...    entity_id=${entity_id}
    ...    filename=${entity_replacement_filename}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Body Containing Entity element
    ...    expectation_filename=${expectation_filename}
    ...    entity_id=${entity_id}
    ...    response_body=${response1.json()}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
