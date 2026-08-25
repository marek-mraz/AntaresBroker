*** Settings ***
Documentation       Check that an entity with observationSpace geospatial Property can be retrieved

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entity
Suite Teardown      Delete Created Entity
Test Template       Retrieve Entity With observationSpace Geospatial Property


*** Variables ***
${filename}=    building-observation-space-geoproperty.jsonld


*** Test Cases ***    OPTIONS    EXPECTATION_FILENAME
018_08_01 Simplified
    [Tags]    e-retrieve    5_7_1    4_7
    keyValues    building-observation-space-geoproperty-simplified.jsonld
018_08_02 Normalized
    [Tags]    e-retrieve    5_7_1    4_7
    ${EMPTY}    building-observation-space-geoproperty-normalized.jsonld


*** Keywords ***
Retrieve Entity With observationSpace Geospatial Property
    [Documentation]    Check that an entity with observationSpace geospatial Property can be retrieved
    [Arguments]    ${options}    ${expectation_filename}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    options=${options}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element    ${expectation_filename}    ${entity_id}    ${response.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
