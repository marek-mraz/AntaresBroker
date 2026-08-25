*** Settings ***
Documentation       Check that one can append an operationSpace geospatial Property to an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=                                building-minimal.jsonld
${operation_space_fragment_filename}=       operation-space-fragment.json
${expectation_filename}=                    building-operation-space-geoproperty-normalized.jsonld


*** Test Cases ***
010_09 Append operationSpace Geospatial Property To An Entity
    [Documentation]    Check that one can append an operationSpace geospatial Property to an entity
    [Tags]    ea-append    5_6_3    4_7
    ${response}=    Append Entity Attributes
    ...    ${entity_id}
    ...    ${operation_space_fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
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
