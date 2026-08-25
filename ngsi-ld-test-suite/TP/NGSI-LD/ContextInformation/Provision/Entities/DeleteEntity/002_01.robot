*** Settings ***
Documentation       Check that one can delete an entity by id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity


*** Test Cases ***
002_01_01 Delete An Entity
    [Documentation]    Check that one can delete an entity by id
    [Tags]    e-delete    5_6_6
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response2}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resource    ${response2.status_code}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    application/ld+json
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${entity_id}
