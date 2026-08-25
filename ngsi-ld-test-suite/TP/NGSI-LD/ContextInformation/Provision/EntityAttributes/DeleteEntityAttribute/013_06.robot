*** Settings ***
Documentation       Check that one can delete an operationSpace geospatial Property from an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=                building-operation-space-geoproperty.jsonld
${expectation_filename}=    building-minimal-compacted.json


*** Test Cases ***
013_06 Delete An operationSpace Geospatial Property From An Entity
    [Documentation]    Check that one can delete an operationSpace geospatial Property from an entity
    [Tags]    ea-delete    5_6_5    4_7
    ${response}=    Delete Entity Attributes
    ...    ${entity_id}
    ...    operationSpace
    ...    ${EMPTY}
    ...    false
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}


*** Keywords ***
Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity
    ...    ${filename}
    ...    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
