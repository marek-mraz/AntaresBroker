*** Settings ***
Documentation       Check that one can create an entity with observationSpace geospatial Property

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=    building-observation-space-geoproperty.jsonld


*** Test Cases ***
001_12 Create Entity With observationSpace Geospatial Property
    [Documentation]    Check that one can create an entity with observationSpace geospatial Property
    [Tags]    e-create    5_6_1    4_7

    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}

    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${entity_id}    ${response.headers}
    ${created_entity}=    Load Test Sample    entities/${filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Created Resource Set To
    ...    created_resource=${created_entity}
    ...    response_body=${response1.json()}


*** Keywords ***
Delete Initial Entity
    Delete Entity    ${entity_id}
