*** Settings ***
Documentation       Check that one can create an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity
Test Template       Create Entity Scenarios


*** Test Cases ***    FILENAME    CONTENT_TYPE
001_01_01 MinimalEntity
    [Tags]    e-create    5_6_1
    building-minimal.json    application/json
001_01_02 EntityWithSimpleProperties
    [Tags]    e-create    5_6_1
    building-simple-attributes.jsonld    application/ld+json
001_01_03 EntityWithRelationshipsProperties
    [Tags]    e-create    5_6_1
    building-relationship-of-property.jsonld    application/ld+json
001_01_04 EntityWithLocationAttribute
    [Tags]    e-create    5_6_1
    building-location-attribute.jsonld    application/ld+json
001_01_05 EntityWithNonCoreGeoProperty
    [Tags]    e-create    5_6_1
    building-non-core-geoproperty-attribute.jsonld    application/ld+json


*** Keywords ***
Create Entity Scenarios
    [Documentation]    Check that one can create an entity
    [Arguments]    ${filename}    ${content_type}
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${content_type}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Headers Containing URI set to    ${entity_id}    ${response.headers}
    ${created_entity}=    Load Test Sample    entities/${filename}    ${entity_id}
    IF    '${content_type}' == 'application/json'
        ${response1}=    Retrieve Entity
        ...    id=${entity_id}
        ...    accept=${content_type}
    END
    IF    '${content_type}' == 'application/ld+json'
        ${response1}=    Retrieve Entity
        ...    id=${entity_id}
        ...    accept=${content_type}
        ...    context=${ngsild_test_suite_context}
    END
    Check Created Resource Set To
    ...    created_resource=${created_entity}
    ...    response_body=${response1.json()}

Delete Initial Entity
    Delete Entity    ${entity_id}
