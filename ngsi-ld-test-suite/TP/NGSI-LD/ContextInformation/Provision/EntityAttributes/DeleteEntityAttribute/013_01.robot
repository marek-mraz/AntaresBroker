*** Settings ***
Documentation       Check that one can delete an attribute from an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entities
Test Template       Delete Attributes


*** Variables ***
${status_code}=     204
${filename}=        vehicle-speed-only-multi-instances.jsonld
${attribute_id}=    speed


*** Test Cases ***    DATASETID    DELETEALL    EXPECTATION_FILENAME
013_01_01 Delete An Attribute With The Default Instance
    [Tags]    ea-delete    5_6_5
    ${EMPTY}    false    vehicle-delete-default-speed.jsonld
013_01_02 Delete An Attribute With A Specific datasetId
    [Tags]    ea-delete    5_6_5
    urn:ngsi-ld:Dataset:gps    false    vehicle-delete-datasetid-speed.jsonld
013_01_03 Delete All Attribute Instances
    [Tags]    ea-delete    5_6_5
    ${EMPTY}    true    vehicle-delete-deleteall-speed.jsonld


*** Keywords ***
Delete Attributes
    [Documentation]    Check that one can delete an attribute from an entity
    [Arguments]    ${datasetId}    ${deleteAll}    ${expectation_filename}
    ${response}=    Delete Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    datasetId=${datasetId}
    ...    deleteAll=${deleteAll}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response2}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ${ignored_attributes}=    Create List    @context
    Check Updated Resource Set To
    ...    updated_resource=${entity_expectation_payload}
    ...    response_body=${response2.json()}
    ...    ignored_keys=${ignored_attributes}

Setup Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Initial Entities
    Delete Entity    ${entity_id}
